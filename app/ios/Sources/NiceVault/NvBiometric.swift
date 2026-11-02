import CryptoKit
import Foundation
import LocalAuthentication
import Security
import Tauri
import UIKit

struct BioArgs: Decodable {
  let alias: String
  let data: String?
  let title: String?
  let cancel: String?
}

extension NvPlugin {
  private static func kind(_ c: LAContext) -> String {
    switch c.biometryType {
    case .faceID: return "face"
    case .touchID: return "fingerprint"
    default: return "biometric"
    }
  }

  private static func keyQuery(_ alias: String) -> [String: Any] {
    [
      kSecClass as String: kSecClassKey,
      kSecAttrApplicationTag as String: Data(alias.utf8),
      kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
    ]
  }

  private static func deleteKey(_ alias: String) {
    SecItemDelete(keyQuery(alias) as CFDictionary)
  }

  @objc public func bioStatus(_ invoke: Invoke) {
    let c = LAContext()
    var err: NSError?
    let ok = c.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &err)
    var reason = ""
    if !SecureEnclave.isAvailable {
      reason = "no_hardware"
    } else if !ok {
      switch LAError.Code(rawValue: err?.code ?? 0) {
      case .biometryNotEnrolled: reason = "none_enrolled"
      case .passcodeNotSet: reason = "no_passcode"
      case .biometryNotAvailable: reason = "no_hardware"
      default: reason = "unavailable"
      }
    }
    invoke.resolve(["available": reason.isEmpty, "kind": NvPlugin.kind(c), "reason": reason, "device": UIDevice.current.model])
  }

  @objc public func bioWrap(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(BioArgs.self), var secret = Data(base64Encoded: args.data ?? "") else {
      invoke.reject("bad arguments")
      return
    }
    defer { secret.resetBytes(in: 0..<secret.count) }
    let c = LAContext()
    var laErr: NSError?
    guard c.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &laErr), let state = c.evaluatedPolicyDomainState else {
      invoke.reject("Face ID / Touch ID is not available.", code: "bio_failed")
      return
    }
    NvPlugin.deleteKey(args.alias)
    var err: Unmanaged<CFError>?
    guard let access = SecAccessControlCreateWithFlags(nil, kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly, [.privateKeyUsage, .biometryCurrentSet], &err) else {
      invoke.reject("access control failed", code: "bio_failed")
      return
    }
    let attrs: [String: Any] = [
      kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
      kSecAttrKeySizeInBits as String: 256,
      kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
      kSecPrivateKeyAttrs as String: [
        kSecAttrIsPermanent as String: true,
        kSecAttrApplicationTag as String: Data(args.alias.utf8),
        kSecAttrAccessControl as String: access,
      ] as [String: Any],
    ]
    guard let priv = SecKeyCreateRandomKey(attrs as CFDictionary, &err), let pub = SecKeyCopyPublicKey(priv),
      let ct = SecKeyCreateEncryptedData(pub, .eciesEncryptionCofactorVariableIVX963SHA256AESGCM, secret as CFData, &err) as Data?
    else {
      NvPlugin.deleteKey(args.alias)
      invoke.reject((err?.takeRetainedValue() as Error?)?.localizedDescription ?? "Secure Enclave failed", code: "no_hardware")
      return
    }
    var blob = Data([UInt8(state.count >> 8), UInt8(state.count & 0xff)])
    blob.append(state)
    blob.append(ct)
    invoke.resolve(["data": blob.base64EncodedString()])
  }

  @objc public func bioUnwrap(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(BioArgs.self), let blob = Data(base64Encoded: args.data ?? ""), blob.count > 2 else {
      invoke.reject("bad arguments")
      return
    }
    let n = Int(blob[blob.startIndex]) << 8 | Int(blob[blob.startIndex + 1])
    guard blob.count > 2 + n else {
      invoke.reject("bad blob", code: "bio_invalidated")
      return
    }
    let saved = blob.subdata(in: (blob.startIndex + 2)..<(blob.startIndex + 2 + n))
    let ct = blob.subdata(in: (blob.startIndex + 2 + n)..<blob.endIndex)
    let c = LAContext()
    c.localizedCancelTitle = args.cancel
    c.localizedReason = args.title ?? ""
    var laErr: NSError?
    if c.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &laErr), let now = c.evaluatedPolicyDomainState, now != saved {
      NvPlugin.deleteKey(args.alias)
      invoke.reject("Face ID / Touch ID changed.", code: "bio_invalidated")
      return
    }
    if LAError.Code(rawValue: laErr?.code ?? 0) == .biometryNotEnrolled {
      NvPlugin.deleteKey(args.alias)
      invoke.reject("Face ID / Touch ID changed.", code: "bio_invalidated")
      return
    }
    if LAError.Code(rawValue: laErr?.code ?? 0) == .biometryLockout {
      invoke.reject("Too many attempts.", code: "bio_lockout")
      return
    }
    var q = NvPlugin.keyQuery(args.alias)
    q[kSecReturnRef as String] = true
    q[kSecUseAuthenticationContext as String] = c
    var item: CFTypeRef?
    guard SecItemCopyMatching(q as CFDictionary, &item) == errSecSuccess, let found = item else {
      invoke.reject("the key is gone", code: "bio_invalidated")
      return
    }
    let key = found as! SecKey
    DispatchQueue.global(qos: .userInitiated).async {
      var err: Unmanaged<CFError>?
      if let out = SecKeyCreateDecryptedData(key, .eciesEncryptionCofactorVariableIVX963SHA256AESGCM, ct as CFData, &err) as Data? {
        var plain = out
        invoke.resolve(["data": plain.base64EncodedString()])
        plain.resetBytes(in: 0..<plain.count)
        return
      }
      let e = err?.takeRetainedValue()
      let code = e.map { CFErrorGetCode($0) } ?? 0
      switch code {
      case LAError.Code.userCancel.rawValue, LAError.Code.appCancel.rawValue, LAError.Code.systemCancel.rawValue,
        LAError.Code.userFallback.rawValue, Int(errSecUserCanceled):
        invoke.reject("cancelled", code: "cancelled")
      case LAError.Code.biometryLockout.rawValue:
        invoke.reject("Too many attempts.", code: "bio_lockout")
      default:
        invoke.reject((e as Error?)?.localizedDescription ?? "Face ID / Touch ID failed.", code: "bio_failed")
      }
    }
  }

  @objc public func bioDelete(_ invoke: Invoke) {
    if let args = try? invoke.parseArgs(BioArgs.self) { NvPlugin.deleteKey(args.alias) }
    invoke.resolve()
  }
}
