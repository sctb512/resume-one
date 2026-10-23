import AppKit
import CoreText
import Foundation

guard CommandLine.arguments.count == 3 else {
    fputs("usage: render-icon.swift font.ttf output.png\n", stderr)
    exit(2)
}

let size = 1024
let fontURL = URL(fileURLWithPath: CommandLine.arguments[1])
let outputURL = URL(fileURLWithPath: CommandLine.arguments[2])
guard let provider = CGDataProvider(url: fontURL as CFURL),
      let graphicsFont = CGFont(provider) else {
    fputs("cannot read bundled icon font\n", stderr)
    exit(3)
}

let colorSpace = CGColorSpaceCreateDeviceRGB()
guard let context = CGContext(
    data: nil, width: size, height: size, bitsPerComponent: 8,
    bytesPerRow: 0, space: colorSpace,
    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
) else {
    fputs("cannot create icon bitmap\n", stderr)
    exit(3)
}

func color(_ red: CGFloat, _ green: CGFloat, _ blue: CGFloat) -> CGColor {
    CGColor(colorSpace: colorSpace, components: [red / 255, green / 255, blue / 255, 1])!
}

context.setFillColor(color(146, 95, 80))
context.addPath(CGPath(roundedRect: CGRect(x: 20, y: 20, width: 984, height: 984),
                       cornerWidth: 215, cornerHeight: 215, transform: nil))
context.fillPath()
context.setStrokeColor(color(213, 174, 139))
context.setLineWidth(9)
context.addPath(CGPath(roundedRect: CGRect(x: 70, y: 70, width: 884, height: 884),
                       cornerWidth: 170, cornerHeight: 170, transform: nil))
context.strokePath()

let font = CTFontCreateWithGraphicsFont(graphicsFont, 620, nil, nil)
let text = NSAttributedString(string: "简", attributes: [
    NSAttributedString.Key(rawValue: kCTFontAttributeName as String): font,
    NSAttributedString.Key(rawValue: kCTForegroundColorAttributeName as String): color(255, 249, 237),
])
let line = CTLineCreateWithAttributedString(text)
var ascent: CGFloat = 0
var descent: CGFloat = 0
let width = CTLineGetTypographicBounds(line, &ascent, &descent, nil)
context.textPosition = CGPoint(x: (CGFloat(size) - width) / 2,
                               y: (CGFloat(size) - ascent - descent) / 2 + descent - 8)
CTLineDraw(line, context)

guard let image = context.makeImage(),
      let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
    fputs("cannot encode icon PNG\n", stderr)
    exit(3)
}
try data.write(to: outputURL, options: .atomic)
