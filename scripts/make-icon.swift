// Turns a square artwork PNG into a macOS-style app icon: the art scaled into the standard
// 824×824 rounded square (corner radius 185) centered on a transparent 1024×1024 canvas.
//
//   swift scripts/make-icon.swift assets/icon-source.png assets/icon.png

import AppKit
import CoreGraphics

let args = CommandLine.arguments
guard args.count == 3 else {
    print("usage: make-icon.swift <source.png> <output.png>")
    exit(1)
}
guard let source = NSImage(contentsOfFile: args[1]),
      let art = source.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    print("couldn't read \(args[1])")
    exit(1)
}

let canvas = 1024
let inset = CGFloat(100)          // (1024 - 824) / 2
let radius = CGFloat(185)
let rect = CGRect(x: inset, y: inset, width: CGFloat(canvas) - 2 * inset, height: CGFloat(canvas) - 2 * inset)

guard let ctx = CGContext(
    data: nil, width: canvas, height: canvas, bitsPerComponent: 8, bytesPerRow: 0,
    space: CGColorSpace(name: CGColorSpace.sRGB)!,
    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
) else { exit(1) }
ctx.interpolationQuality = .high
ctx.addPath(CGPath(roundedRect: rect, cornerWidth: radius, cornerHeight: radius, transform: nil))
ctx.clip()
ctx.draw(art, in: rect)

guard let out = ctx.makeImage(),
      let png = NSBitmapImageRep(cgImage: out).representation(using: .png, properties: [:]) else {
    exit(1)
}
try png.write(to: URL(fileURLWithPath: args[2]))
print("wrote \(args[2])")
