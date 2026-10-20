// Draws Show Mode's Windows icons and writes them into Resources/: the app icon
// (ShowMode.ico) and the notification-area icons (tray/*.png), which the app embeds and
// loads at whatever size the taskbar asks for. The masks are the same drawing as the macOS
// app icon (showmode's scripts/make-icon.swift), redrawn rather than taken from SF Symbols,
// whose licence doesn't allow them in app icons.
//
//   swift scripts/make-icons.swift
import AppKit

let navy = NSColor(srgbRed: 0x0f / 255, green: 0x1b / 255, blue: 0x2d / 255, alpha: 1)
let navyLight = NSColor(srgbRed: 0x1d / 255, green: 0x33 / 255, blue: 0x52 / 255, alpha: 1)
let paper = NSColor(srgbRed: 0xf5 / 255, green: 0xf5 / 255, blue: 0xf4 / 255, alpha: 1)
let cyan = NSColor(srgbRed: 0x4c / 255, green: 0xc9 / 255, blue: 0xf0 / 255, alpha: 1)

/// A mask in its own unit space: 1 wide, about 1.2 tall, y up, chin at the bottom.
func maskOutline() -> NSBezierPath {
    let p = NSBezierPath()
    p.move(to: NSPoint(x: 0.10, y: 1.18))
    p.curve(to: NSPoint(x: 0.90, y: 1.18), controlPoint1: NSPoint(x: 0.35, y: 1.24), controlPoint2: NSPoint(x: 0.65, y: 1.24))
    p.curve(to: NSPoint(x: 1.00, y: 1.06), controlPoint1: NSPoint(x: 0.96, y: 1.17), controlPoint2: NSPoint(x: 1.00, y: 1.13))
    p.curve(to: NSPoint(x: 0.50, y: 0.00), controlPoint1: NSPoint(x: 1.00, y: 0.50), controlPoint2: NSPoint(x: 0.86, y: 0.00))
    p.curve(to: NSPoint(x: 0.00, y: 1.06), controlPoint1: NSPoint(x: 0.14, y: 0.00), controlPoint2: NSPoint(x: 0.00, y: 0.50))
    p.curve(to: NSPoint(x: 0.10, y: 1.18), controlPoint1: NSPoint(x: 0.00, y: 1.13), controlPoint2: NSPoint(x: 0.04, y: 1.17))
    p.close()
    return p
}

/// Comedy: eyes squeezed shut in arcs, and a wide open smile.
func comedyFeatures() -> [(NSBezierPath, Bool)] {
    var out: [(NSBezierPath, Bool)] = []
    for x in [0.30, 0.70] {
        let eye = NSBezierPath()
        eye.appendArc(withCenter: NSPoint(x: x, y: 0.74), radius: 0.11, startAngle: 20, endAngle: 160)
        eye.lineWidth = 0.075
        out.append((eye, false))
    }
    let mouth = NSBezierPath()
    mouth.move(to: NSPoint(x: 0.20, y: 0.50))
    mouth.curve(to: NSPoint(x: 0.80, y: 0.50), controlPoint1: NSPoint(x: 0.40, y: 0.45), controlPoint2: NSPoint(x: 0.60, y: 0.45))
    mouth.curve(to: NSPoint(x: 0.20, y: 0.50), controlPoint1: NSPoint(x: 0.74, y: 0.18), controlPoint2: NSPoint(x: 0.26, y: 0.18))
    mouth.close()
    out.append((mouth, true))
    return out
}

/// Tragedy: round eyes and a downturned mouth.
func tragedyFeatures() -> [(NSBezierPath, Bool)] {
    var out: [(NSBezierPath, Bool)] = []
    for x in [0.31, 0.69] {
        out.append((NSBezierPath(ovalIn: NSRect(x: x - 0.075, y: 0.66, width: 0.15, height: 0.15)), true))
    }
    let mouth = NSBezierPath()
    mouth.appendArc(withCenter: NSPoint(x: 0.5, y: 0.14), radius: 0.28, startAngle: 50, endAngle: 130)
    mouth.lineWidth = 0.075
    out.append((mouth, false))
    return out
}

struct Placement {
    let centre: NSPoint
    let width: CGFloat
    let degrees: CGFloat

    var transform: NSAffineTransform {
        let t = NSAffineTransform()
        t.translateX(by: centre.x, yBy: centre.y)
        t.rotate(byDegrees: degrees)
        t.scale(by: width)
        t.translateX(by: -0.5, yBy: -0.6)
        return t
    }
}

/// Tragedy behind, comedy in front, the same arrangement as the macOS icon.
let tragedy = Placement(centre: NSPoint(x: 648, y: 462), width: 318, degrees: -16)
let comedy = Placement(centre: NSPoint(x: 382, y: 566), width: 336, degrees: 12)

/// Strokes or fills each feature through the mask's transform.
func drawFeatures(_ features: [(NSBezierPath, Bool)], _ at: Placement, scale: CGFloat = 1) {
    for (path, filled) in features {
        let p = at.transform.transform(path)
        if filled { p.fill() } else {
            p.lineWidth = path.lineWidth * at.width * scale
            p.lineCapStyle = .round
            p.stroke()
        }
    }
}

func bitmap(_ px: Int, _ draw: () -> Void) -> NSBitmapImageRep {
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px, bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: 1024, height: 1024)   // draw in 1024-point space at any pixel size
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.imageInterpolation = .high
    draw()
    NSGraphicsContext.restoreGraphicsState()
    return rep
}

/// The app icon: the masks on a navy tile, filling more of the square than the macOS grid
/// does, since Windows draws icons without a tile of its own.
func appIcon(_ px: Int) -> NSBitmapImageRep {
    bitmap(px) {
        let tile = NSBezierPath(roundedRect: NSRect(x: 24, y: 24, width: 976, height: 976), xRadius: 180, yRadius: 180)
        NSGradient(starting: navyLight, ending: navy)!.draw(in: tile, angle: -90)
        for (features, at, fill) in [(tragedyFeatures(), tragedy, cyan), (comedyFeatures(), comedy, paper)] {
            let outline = at.transform.transform(maskOutline())
            navy.setStroke()
            outline.lineWidth = 44
            outline.lineJoinStyle = .round
            outline.stroke()
            fill.setFill()
            outline.fill()
            navy.set()
            drawFeatures(features, at)
        }
    }
}

/// A notification-area icon: one colour on transparent. Off is the masks in outline, on is
/// them filled with the features cut out, the way the macOS menu bar shows the two states.
/// Lines are kept at least a pixel and a bit wide so 16 px still reads.
func trayIcon(_ px: Int, filled: Bool, colour: NSColor) -> NSBitmapImageRep {
    bitmap(px) {
        let ctx = NSGraphicsContext.current!
        let pixel = 1024 / CGFloat(px)
        let line = max(1.3 * pixel, 52)
        let gap = max(1.6 * pixel, 56)
        // Zoom in on the two masks: they sit within roughly 160...880 of the 1024 square.
        let zoom = NSAffineTransform()
        zoom.translateX(by: 512, yBy: 512)
        zoom.scale(by: 1.32)
        zoom.translateX(by: -512, yBy: -534)
        zoom.concat()
        for (features, at) in [(tragedyFeatures(), tragedy), (comedyFeatures(), comedy)] {
            let outline = at.transform.transform(maskOutline())
            if at.centre == comedy.centre {
                // Clear a gap around the front mask so the two read as separate shapes.
                ctx.compositingOperation = .clear
                outline.lineWidth = gap / 1.32
                outline.lineJoinStyle = .round
                outline.stroke()
                outline.fill()
                ctx.compositingOperation = .sourceOver
            }
            colour.set()
            if filled {
                outline.fill()
                ctx.compositingOperation = .clear
                drawFeatures(features, at, scale: max(1, line / 1.32 / (0.075 * at.width)))
                ctx.compositingOperation = .sourceOver
            } else {
                outline.lineWidth = line / 1.32
                outline.lineJoinStyle = .round
                outline.stroke()
                drawFeatures(features, at, scale: max(1, line / 1.32 / (0.075 * at.width)))
            }
        }
    }
}

func png(_ rep: NSBitmapImageRep) -> Data { rep.representation(using: .png, properties: [:])! }

/// An .ico whose entries are PNGs, which every Windows since Vista reads.
func ico(_ images: [(Int, Data)]) -> Data {
    var d = Data()
    func u16(_ v: Int) { var x = UInt16(v).littleEndian; d.append(Data(bytes: &x, count: 2)) }
    func u32(_ v: Int) { var x = UInt32(v).littleEndian; d.append(Data(bytes: &x, count: 4)) }
    u16(0); u16(1); u16(images.count)
    var offset = 6 + 16 * images.count
    for (px, data) in images {
        d.append(UInt8(px >= 256 ? 0 : px)); d.append(UInt8(px >= 256 ? 0 : px))
        d.append(0); d.append(0)
        u16(1); u16(32)
        u32(data.count); u32(offset)
        offset += data.count
    }
    for (_, data) in images { d.append(data) }
    return d
}

let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
let resources = root.appendingPathComponent("Resources")
let trayDir = resources.appendingPathComponent("tray")
try! FileManager.default.createDirectory(at: trayDir, withIntermediateDirectories: true)

let appSizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
try! ico(appSizes.map { ($0, png(appIcon($0))) }).write(to: resources.appendingPathComponent("ShowMode.ico"))

// "light" and "dark" name the taskbar the icon sits on.
let ink = NSColor(srgbRed: 0x1c / 255, green: 0x1c / 255, blue: 0x1c / 255, alpha: 1)
for (theme, colour) in [("light", ink), ("dark", NSColor.white)] {
    for (state, filled) in [("off", false), ("on", true)] {
        for px in [16, 20, 24, 32, 40, 48] {
            try! png(trayIcon(px, filled: filled, colour: colour))
                .write(to: trayDir.appendingPathComponent("\(state)-\(theme)-\(px).png"))
        }
    }
}
print("wrote \(resources.path)/ShowMode.ico and \(trayDir.path)")
