// NiceVault's own bridge to iOS (app/src-tauri/src/mobile.rs), the twin
// of the Android NvPlugin.kt:
//  - pickPhotos / loadPhoto / cancelPhotos: the camera roll through PHPicker (photos and videos,
//    several at once, no photo-library permission). The pick is kept here; mobile.rs loads its
//    items one by one, four at a time (an iCloud-only photo is downloaded first, its progress
//    reported through nv_mobile_pick_progress), each copied into a staging folder Rust gives,
//    under its original name, with the original bytes (HEIC / HEVC as they are);
//  - pickDocuments: the document picker opened in place (security-scoped URLs, no copy left in
//    the app's Inbox), copied into the staging folder under the file's real name;
//  - displayNames, share: the names of file URLs, the share sheet (UIActivityViewController)
//    for "Condividi" / "Apri con";
//  - privacy: a cover over the window while the app is not active (app switcher snapshot,
//    Control Center, Siri) or the screen is being recorded / mirrored, and every open volume
//    locked after 30 s in the background (nv_mobile_lock_all in mobile.rs), before the cover
//    comes off.
import Photos
import PhotosUI
import Tauri
import UIKit
import UniformTypeIdentifiers
import WebKit

@_silgen_name("nv_mobile_lock_all")
func nvMobileLockAll()

@_silgen_name("nv_mobile_pick_progress")
func nvMobilePickProgress(_ index: Int, _ fraction: Double)

struct LoadArgs: Decodable {
  let session: Int
  let index: Int
  let dir: String
}

/// A download (iCloud) that makes no progress this long is given up and retried by mobile.rs.
let stallSeconds: TimeInterval = 180

struct PickArgs: Decodable {
  let dir: String
  let multiple: Bool
}

struct UrisArgs: Decodable {
  let uris: [String]
}

struct GalleryArgs: Decodable {
  let path: String
  let video: Bool
}

struct ShareArgs: Decodable {
  let paths: [String]
  let title: String?
}

/// In the background this long: every open volume is locked (= BACKGROUND_LOCK_MS, lib/mobile.ts).
let backgroundLockSeconds: TimeInterval = 30
/// A page that doesn't answer this long after a return to the foreground is loaded again.
let pageAnswerSeconds: TimeInterval = 2.5
/// The privacy cover never stays longer than this while locking.
let coverMaxSeconds: TimeInterval = 8

class NvPlugin: Plugin, PHPickerViewControllerDelegate, UIDocumentPickerDelegate, UIAdaptivePresentationControllerDelegate {
  private var pending: Invoke?
  /// The last camera-roll pick (mobile.rs loads its items one by one) and the loads in flight.
  private let photoLock = NSLock()
  private var picked: [PHPickerResult] = []
  private var photoSession = 0
  private var cancelledSession = -1
  private var loads: [Int: Progress] = [:]
  private var pendingDir: URL?
  private weak var window: UIWindow?
  private var cover: UIView?
  private var backgroundSince: Date?
  private var backgroundTask: UIBackgroundTaskIdentifier = .invalid
  private var locking = false
  private weak var webView: WKWebView?
  /// Probes of the page after a return to the foreground (a newer one supersedes the older).
  private var probeSeq = 0
  /// The app went to the background since it was last active (a probe is due on return).
  private var wasInBackground = false
  /// The latest lockAll (its cover cap applies to it only).
  private var lockGen = 0

  // MARK: pickers

  private func begin(_ invoke: Invoke) -> PickArgs? {
    guard let args = try? invoke.parseArgs(PickArgs.self) else {
      invoke.reject("bad arguments")
      return nil
    }
    if let old = pending { old.resolve(["files": [String]()]) }
    pending = invoke
    pendingDir = URL(fileURLWithPath: args.dir, isDirectory: true)
    return args
  }

  private func finish(_ files: [String], error: String? = nil) {
    guard let invoke = pending else { return }
    pending = nil
    pendingDir = nil
    if let error = error { invoke.reject(error) } else { invoke.resolve(["files": files]) }
  }

  private func present(_ vc: UIViewController) {
    vc.presentationController?.delegate = self
    guard let root = manager.viewController else {
      finish([], error: "no view controller")
      return
    }
    var top = root
    while let p = top.presentedViewController { top = p }
    top.present(vc, animated: true)
  }

  /// `dir/<k>/<name>`: each pick in its own folder (two may share a name).
  private static func slot(_ dir: URL, _ k: Int, _ name: String) throws -> URL {
    let sub = dir.appendingPathComponent(String(k), isDirectory: true)
    try FileManager.default.createDirectory(at: sub, withIntermediateDirectories: true)
    var n = name.replacingOccurrences(of: "/", with: "_").trimmingCharacters(in: .whitespacesAndNewlines)
    if n.isEmpty || n == "." || n == ".." { n = "file" }
    return sub.appendingPathComponent(n, isDirectory: false)
  }

  @objc public func pickPhotos(_ invoke: Invoke) {
    // called on Tauri's IPC queue: the picker state lives on the main thread
    DispatchQueue.main.async {
      guard let args = self.begin(invoke) else { return }
      // no photoLibrary: out of process, no permission prompt, no asset identifiers
      var config = PHPickerConfiguration()
      config.selectionLimit = args.multiple ? 0 : 1
      config.filter = .any(of: [.images, .videos])
      // the original file (HEIC, HEVC, ProRAW...), never a compatible re-encode
      config.preferredAssetRepresentationMode = .current
      let picker = PHPickerViewController(configuration: config)
      picker.delegate = self
      self.present(picker)
    }
  }

  func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
    picker.dismiss(animated: true)
    guard let invoke = pending else { return }
    pending = nil
    pendingDir = nil
    // nothing is loaded here: mobile.rs asks for each item (loadPhoto), four at a time, and
    // imports it as soon as it is there (an iCloud-only photo is downloaded first)
    photoLock.lock()
    for (_, p) in loads { p.cancel() }
    loads.removeAll()
    photoSession += 1
    picked = results
    let session = photoSession
    photoLock.unlock()
    let names = results.map { $0.itemProvider.suggestedName ?? "" }
    invoke.resolve(["session": session, "names": names])
  }

  /// The original representation: the first registered type that is a picture or a movie.
  private static func mediaType(_ p: NSItemProvider) -> String? {
    p.registeredTypeIdentifiers.first { id in
      guard let t = UTType(id) else { return false }
      return t.conforms(to: .image) || t.conforms(to: .movie) || t.conforms(to: .audiovisualContent)
    }
  }

  /// What went wrong, as a code mobile.rs / pick_queue.rs knows (offline, icloud, stalled,
  /// unsupported, no_space, cancelled, other); looks through the underlying errors.
  private static func errorCode(_ error: Error?) -> String {
    var e = error as NSError?
    var depth = 0
    var sawPhotos = false
    while let n = e, depth < 6 {
      switch n.domain {
      case NSURLErrorDomain:
        let offline = [NSURLErrorNotConnectedToInternet, NSURLErrorNetworkConnectionLost, NSURLErrorTimedOut,
                       NSURLErrorCannotConnectToHost, NSURLErrorDataNotAllowed, NSURLErrorInternationalRoamingOff,
                       NSURLErrorCannotFindHost, NSURLErrorDNSLookupFailed]
        if offline.contains(n.code) { return "offline" }
      case NSCocoaErrorDomain:
        if n.code == NSFileWriteOutOfSpaceError { return "no_space" }
        if n.code == NSUserCancelledError { return "cancelled" }
      case NSPOSIXErrorDomain:
        if n.code == Int(ENOSPC) { return "no_space" }
      case "PHPhotosErrorDomain":
        // 3164 networkAccessRequired, 3169 networkError
        if n.code == 3164 || n.code == 3169 { return "offline" }
        sawPhotos = true
      default:
        if n.domain.contains("CloudPhoto") || n.domain.hasPrefix("CPL") { sawPhotos = true }
      }
      e = n.userInfo[NSUnderlyingErrorKey] as? NSError
      depth += 1
    }
    return sawPhotos ? "icloud" : "other"
  }

  /// One picked item into `dir` (mobile.rs pick queue). The photo library downloads it from
  /// iCloud first when the phone has only an optimised copy (the picker's results come with
  /// network access): this waits for it however long it takes while it makes progress,
  /// reporting the fraction; no progress for `stallSeconds` = "stalled" (retried by Rust).
  @objc public func loadPhoto(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(LoadArgs.self) else {
      invoke.reject("bad arguments")
      return
    }
    photoLock.lock()
    let current = args.session == photoSession && args.index >= 0 && args.index < picked.count
    let result = current ? picked[args.index] : nil
    photoLock.unlock()
    guard let r = result else {
      invoke.reject("the picked photos are no longer available", code: "expired")
      return
    }
    let p = r.itemProvider
    guard let type = NvPlugin.mediaType(p) else {
      invoke.reject("not a picture or a movie", code: "unsupported")
      return
    }
    let index = args.index
    let dir = URL(fileURLWithPath: args.dir, isDirectory: true)
    var stalled = false
    var finished = false
    // the URL is a temporary copy deleted when the handler returns: copied here
    let progress = p.loadFileRepresentation(forTypeIdentifier: type) { [weak self] url, error in
      self?.photoLock.lock()
      finished = true
      self?.loads[index] = nil
      let wasStalled = stalled
      let cancelled = self?.cancelledSession == args.session
      self?.photoLock.unlock()
      guard let url = url else {
        let code = wasStalled ? "stalled" : (cancelled ? "cancelled" : NvPlugin.errorCode(error))
        invoke.reject(error?.localizedDescription ?? "the photo could not be read", code: code)
        return
      }
      var ext = url.pathExtension
      if ext.isEmpty { ext = UTType(type)?.preferredFilenameExtension ?? "" }
      let stem = (p.suggestedName?.isEmpty == false ? p.suggestedName! : url.deletingPathExtension().lastPathComponent)
      let name = ext.isEmpty || stem.lowercased().hasSuffix("." + ext.lowercased()) ? stem : stem + "." + ext
      do {
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        var n = name.replacingOccurrences(of: "/", with: "_").trimmingCharacters(in: .whitespacesAndNewlines)
        if n.isEmpty || n == "." || n == ".." { n = "file" }
        let dest = dir.appendingPathComponent(n, isDirectory: false)
        try FileManager.default.copyItem(at: url, to: dest)
        invoke.resolve(["path": dest.path])
      } catch {
        invoke.reject(error.localizedDescription, code: NvPlugin.errorCode(error))
      }
    }
    photoLock.lock()
    loads[index] = progress
    photoLock.unlock()
    // the fraction for "Scarico da iCloud… N di M", and the stall watch
    var last = -1.0
    var lastMove = Date()
    func tick() {
      photoLock.lock()
      let done = finished
      photoLock.unlock()
      if done { return }
      let f = progress.fractionCompleted
      nvMobilePickProgress(index, f)
      if f != last {
        last = f
        lastMove = Date()
      } else if Date().timeIntervalSince(lastMove) > stallSeconds {
        photoLock.lock()
        stalled = true
        photoLock.unlock()
        progress.cancel()
        return
      }
      DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 0.25) { tick() }
    }
    DispatchQueue.global(qos: .utility).async { tick() }
  }

  /// Annulla: the downloads in flight stop now.
  @objc public func cancelPhotos(_ invoke: Invoke) {
    photoLock.lock()
    cancelledSession = photoSession
    let all = Array(loads.values)
    photoLock.unlock()
    for p in all { p.cancel() }
    invoke.resolve()
  }

  @objc public func pickDocuments(_ invoke: Invoke) {
    // called on Tauri's IPC queue: the picker state lives on the main thread
    DispatchQueue.main.async {
      guard let args = self.begin(invoke) else { return }
      // asCopy false: open in place (security-scoped), so iOS keeps no copy in tmp/<id>-Inbox
      let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.item], asCopy: false)
      picker.allowsMultipleSelection = args.multiple
      picker.delegate = self
      self.present(picker)
    }
  }

  func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
    guard let dir = pendingDir else { return }
    DispatchQueue.global(qos: .userInitiated).async {
      var files: [String] = []
      var failure: String?
      for (k, url) in urls.enumerated() {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        // coordinated: a file in iCloud Drive or another provider is downloaded first
        var err: NSError?
        var copyError: Error?
        NSFileCoordinator().coordinate(readingItemAt: url, options: [], error: &err) { u in
          do {
            let dest = try NvPlugin.slot(dir, k, url.lastPathComponent)
            try FileManager.default.copyItem(at: u, to: dest)
            files.append(dest.path)
          } catch {
            copyError = error
          }
        }
        if let e = err ?? copyError { failure = failure ?? e.localizedDescription }
      }
      DispatchQueue.main.async {
        if files.isEmpty, let failure = failure { self.finish([], error: failure) } else { self.finish(files) }
      }
    }
  }

  func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
    finish([])
  }

  /// Swiped down without choosing.
  func presentationControllerDidDismiss(_ presentationController: UIPresentationController) {
    finish([])
  }

  @objc public func displayNames(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(UrisArgs.self)
    let names = args.uris.map { URL(string: $0)?.lastPathComponent ?? "" }
    invoke.resolve(["names": names])
  }

  // MARK: gallery

  /// "Salva nella Galleria" (mobile.rs mobile_save_gallery): the decrypted copy Rust staged in
  /// <cache>/tmp/nv-gallery is MOVED into the photo library (shouldMoveFile), add-only access
  /// (NSPhotoLibraryAddUsageDescription; iOS asks once). Rust wipes the staging folder after.
  @objc public func saveToGallery(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(GalleryArgs.self)
    let url = URL(fileURLWithPath: args.path)
    let save = {
      PHPhotoLibrary.shared().performChanges({
        let req = PHAssetCreationRequest.forAsset()
        let opts = PHAssetResourceCreationOptions()
        opts.shouldMoveFile = true
        opts.originalFilename = url.lastPathComponent
        req.addResource(with: args.video ? .video : .photo, fileURL: url, options: opts)
      }) { ok, error in
        if ok {
          invoke.resolve()
        } else {
          invoke.reject(error?.localizedDescription ?? "The photo library refused the file.", code: "gallery_failed")
        }
      }
    }
    let denied = { invoke.reject("NiceVault may not add to Photos (Settings > NiceVault > Photos).", code: "gallery_denied") }
    if #available(iOS 14, *) {
      switch PHPhotoLibrary.authorizationStatus(for: .addOnly) {
      case .authorized, .limited: save()
      case .notDetermined:
        PHPhotoLibrary.requestAuthorization(for: .addOnly) { st in
          if st == .authorized || st == .limited { save() } else { denied() }
        }
      default: denied()
      }
    } else {
      switch PHPhotoLibrary.authorizationStatus() {
      case .authorized: save()
      case .notDetermined:
        PHPhotoLibrary.requestAuthorization { st in
          if st == .authorized { save() } else { denied() }
        }
      default: denied()
      }
    }
  }

  // MARK: share sheet

  @objc public func share(_ invoke: Invoke) throws {
    let args = try invoke.parseArgs(ShareArgs.self)
    let urls = args.paths.map { URL(fileURLWithPath: $0) }
    DispatchQueue.main.async {
      guard let root = self.manager.viewController else {
        invoke.reject("no view controller")
        return
      }
      var top = root
      while let p = top.presentedViewController { top = p }
      let sheet = UIActivityViewController(activityItems: urls, applicationActivities: nil)
      // the decrypted copies go as soon as the sheet is done (the receiver has its own copy);
      // what is left is wiped by mobile.rs ten minutes later / at the next start
      sheet.completionWithItemsHandler = { _, _, _, _ in
        for u in urls { try? FileManager.default.removeItem(at: u.deletingLastPathComponent()) }
      }
      if let pop = sheet.popoverPresentationController {
        // iPad: a popover anchored at the bottom centre
        pop.sourceView = top.view
        pop.sourceRect = CGRect(x: top.view.bounds.midX, y: top.view.bounds.maxY - 80, width: 1, height: 1)
        pop.permittedArrowDirections = []
      }
      top.present(sheet, animated: true)
      invoke.resolve()
    }
  }

  // MARK: privacy cover and background lock

  override func load(webview: WKWebView) {
    let nc = NotificationCenter.default
    nc.addObserver(self, selector: #selector(willResignActive), name: UIApplication.willResignActiveNotification, object: nil)
    nc.addObserver(self, selector: #selector(didBecomeActive), name: UIApplication.didBecomeActiveNotification, object: nil)
    nc.addObserver(self, selector: #selector(didEnterBackground), name: UIApplication.didEnterBackgroundNotification, object: nil)
    nc.addObserver(self, selector: #selector(willEnterForeground), name: UIApplication.willEnterForegroundNotification, object: nil)
    nc.addObserver(self, selector: #selector(captureChanged), name: UIScreen.capturedDidChangeNotification, object: nil)
    webView = webview
    DispatchQueue.main.async {
      self.window = webview.window
      self.captureChanged()
    }
  }

  private func coverWindow() -> UIWindow? {
    if let w = window ?? manager.viewController?.view.window { return w }
    return UIApplication.shared.connectedScenes.compactMap { ($0 as? UIWindowScene)?.windows.first { $0.isKeyWindow } }.first
  }

  private func showCover() {
    guard cover == nil, let w = coverWindow() else { return }
    let v = UIView(frame: w.bounds)
    v.autoresizingMask = [.flexibleWidth, .flexibleHeight]
    // the app's anthracite, the name in a Didone (the Bodoni of the UI): nothing of the page
    v.backgroundColor = UIColor(red: 0.106, green: 0.106, blue: 0.114, alpha: 1)
    let label = UILabel()
    label.text = "NiceVault"
    label.font = UIFont(name: "Didot", size: 34) ?? UIFont.systemFont(ofSize: 34, weight: .light)
    label.textColor = UIColor(white: 0.86, alpha: 1)
    label.translatesAutoresizingMaskIntoConstraints = false
    v.addSubview(label)
    NSLayoutConstraint.activate([
      label.centerXAnchor.constraint(equalTo: v.centerXAnchor),
      label.centerYAnchor.constraint(equalTo: v.centerYAnchor),
    ])
    w.addSubview(v)
    cover = v
  }

  private func hideCover() {
    // kept while locking, or while the screen is recorded / mirrored
    if locking || UIScreen.main.isCaptured || UIApplication.shared.applicationState != .active { return }
    cover?.removeFromSuperview()
    cover = nil
  }

  @objc func willResignActive() { showCover() }

  @objc func didBecomeActive() {
    hideCover()
    // only after a real stay in the background (the web content may have been killed there):
    // a passing inactive state (Control Center, a system sheet, the Passwords AutoFill / Face ID
    // over a password field) leaves the page alive, and a reload there would throw the person
    // back to Benvenuto in the middle of typing; lib.rs reloads a killed page in any case
    guard wasInBackground else { return }
    wasInBackground = false
    reviveWebView()
    probePage()
  }

  /// Back from a long stay in the background, WebKit may keep the page suspended for good: when
  /// the background time runs out while it holds its process assertions it drops every activity of
  /// the page, "View is visible" included, and nothing brings that one back on return (the view
  /// never stopped being in its window), so the layers stay hidden: the grey screen (device log
  /// 2026-10-25). Taking the web view out of its superview and putting it back at the same place
  /// makes WebKit recompute the page's whole activity state (didMoveToWindow), visible again.
  private func reviveWebView() {
    guard let wv = webView, let sv = wv.superview else { return }
    let index = sv.subviews.firstIndex(of: wv) ?? sv.subviews.count
    wv.removeFromSuperview()
    sv.insertSubview(wv, at: min(index, sv.subviews.count))
  }

  /// Back in the foreground: the page must answer. iOS may have killed the web content process
  /// in the background (the view stays blank, grey, and never comes back by itself; lib.rs
  /// catches the termination when WebKit reports it) or the page may be stuck: then every volume
  /// is locked and the app is loaded again (Benvenuto / Sblocca), within a couple of seconds.
  private func probePage() {
    guard let wv = webView else { return }
    probeSeq += 1
    let seq = probeSeq
    var answered = false
    if wv.url == nil {
      recoverPage()
      return
    }
    wv.evaluateJavaScript("document.readyState") { [weak self] _, error in
      answered = true
      guard let self = self, seq == self.probeSeq else { return }
      if error != nil { self.recoverPage() }
    }
    DispatchQueue.main.asyncAfter(deadline: .now() + pageAnswerSeconds) { [weak self] in
      guard let self = self, seq == self.probeSeq, !answered,
            UIApplication.shared.applicationState == .active else { return }
      self.recoverPage()
    }
  }

  private func recoverPage() {
    probeSeq += 1
    lockAll { [weak self] in self?.webView?.reload() }
  }

  @objc func captureChanged() {
    if UIScreen.main.isCaptured { showCover() } else { hideCover() }
  }

  @objc func didEnterBackground() {
    showCover()
    wasInBackground = true
    backgroundSince = Date()
    // iOS lets a background task run for a while: lock when the 30 s are up if it still runs
    endBackgroundTask()
    backgroundTask = UIApplication.shared.beginBackgroundTask(withName: "nv-background-lock") { [weak self] in
      self?.endBackgroundTask()
    }
    let since = backgroundSince
    DispatchQueue.main.asyncAfter(deadline: .now() + backgroundLockSeconds) { [weak self] in
      guard let self = self, self.backgroundSince == since, since != nil else { return }
      self.lockAll { self.endBackgroundTask() }
    }
  }

  @objc func willEnterForeground() {
    let since = backgroundSince
    backgroundSince = nil
    endBackgroundTask()
    // the timer may not have run (the app was suspended): lock before the cover comes off
    if let since = since, Date().timeIntervalSince(since) >= backgroundLockSeconds {
      lockAll {}
    }
  }

  private func lockAll(_ done: @escaping () -> Void) {
    locking = true
    showCover()
    // never a cover for good: a lock that takes too long (vault_lock waits up to 5 s per volume
    // for a reader) still ends with the page on screen; the volumes are unregistered by then
    lockGen += 1
    let gen = lockGen
    DispatchQueue.main.asyncAfter(deadline: .now() + coverMaxSeconds) { [weak self] in
      guard let self = self, self.locking, gen == self.lockGen else { return }
      self.locking = false
      self.hideCover()
    }
    DispatchQueue.global(qos: .userInitiated).async {
      nvMobileLockAll()
      DispatchQueue.main.async {
        self.locking = false
        self.hideCover()
        done()
      }
    }
  }

  private func endBackgroundTask() {
    if backgroundTask != .invalid {
      UIApplication.shared.endBackgroundTask(backgroundTask)
      backgroundTask = .invalid
    }
  }
}

@_cdecl("init_plugin_nvmobile")
func initPlugin() -> Plugin {
  return NvPlugin()
}
