// Thumbnails of pictures the Rust side cannot decode (HEIC/HEIF, AVIF, TIFF, DNG): ImageIO makes
// them (src-tauri/src/thumbs.rs `native`), so the web view gets a small SDR JPEG, never the 12 MP
// HEIC original (decoded at full size by WebKit for every grid tile, and drawn into the glyph
// decode's canvas). Memory only: the bytes come in and go out as buffers, nothing touches disk.
import Foundation
import ImageIO

/// A JPEG of `src` whose longer side is at most `px` (EXIF orientation applied), or nil. The
/// returned buffer (`*outLen` bytes) is freed with `nv_imageio_free`.
@_cdecl("nv_imageio_thumbnail")
public func nv_imageio_thumbnail(_ src: UnsafePointer<UInt8>?, _ len: Int, _ px: UInt32, _ outLen: UnsafeMutablePointer<Int>?) -> UnsafeMutablePointer<UInt8>? {
    guard let src = src, len > 0, px > 0, let outLen = outLen else { return nil }
    return autoreleasepool { () -> UnsafeMutablePointer<UInt8>? in
        let data = Data(bytesNoCopy: UnsafeMutableRawPointer(mutating: src), count: len, deallocator: .none) as CFData
        guard let source = CGImageSourceCreateWithData(data, [kCGImageSourceShouldCache: false] as CFDictionary) else { return nil }
        let opts: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: Int(px),
            kCGImageSourceShouldCacheImmediately: true,
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, opts as CFDictionary) else { return nil }
        let out = NSMutableData()
        guard let dest = CGImageDestinationCreateWithData(out as CFMutableData, "public.jpeg" as CFString, 1, nil) else { return nil }
        CGImageDestinationAddImage(dest, image, [kCGImageDestinationLossyCompressionQuality: 0.86] as CFDictionary)
        guard CGImageDestinationFinalize(dest), out.length > 0 else { return nil }
        let n = out.length
        let buf = UnsafeMutablePointer<UInt8>.allocate(capacity: n)
        out.getBytes(buf, length: n)
        outLen.pointee = n
        return buf
    }
}

@_cdecl("nv_imageio_free")
public func nv_imageio_free(_ p: UnsafeMutablePointer<UInt8>?, _ len: Int) {
    p?.deallocate()
}
