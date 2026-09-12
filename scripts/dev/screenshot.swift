// Dev helper: render a URL (or a local HTML/SVG file) with WebKit and save a
// PNG, so the notebook's actual appearance can be inspected from a terminal.
//
//   screenshot <url> <out.png> [waitSeconds] [js] [jsResultFile] [jsBeforeCapture]
//
// Uses WKWebView.createPDF (works offscreen) and converts the result with sips.

import Cocoa
import WebKit

let args = CommandLine.arguments
guard args.count >= 3 else {
    FileHandle.standardError.write(Data("usage: screenshot <url> <out.png> [wait] [js]\n".utf8))
    exit(2)
}
let target = args[1]
let outPath = args[2]
let waitSeconds = args.count > 3 ? (Double(args[3]) ?? 6) : 6
let js = args.count > 4 ? args[4] : ""
let jsResultPath = args.count > 5 ? args[5] : ""
let js2 = args.count > 6 ? args[6] : ""

final class Shooter: NSObject, WKNavigationDelegate {
    var web: WKWebView!
    var window: NSWindow!
    let js2 = CommandLine.arguments.count > 6 ? CommandLine.arguments[6] : ""

    func start() {
        let cfg = WKWebViewConfiguration()
        cfg.websiteDataStore = .nonPersistent()
        let frame = NSRect(x: 0, y: 0, width: 1180, height: 900)
        web = WKWebView(frame: frame, configuration: cfg)
        web.navigationDelegate = self
        window = NSWindow(contentRect: frame,
                          styleMask: [.titled, .resizable],
                          backing: .buffered, defer: false)
        window.contentView = web
        window.alphaValue = 0.0          // invisible, but still rendered
        window.orderFrontRegardless()

        if target.hasPrefix("http://") || target.hasPrefix("https://") {
            web.load(URLRequest(url: URL(string: target)!))
        } else {
            let html = try! String(contentsOfFile: target, encoding: .utf8)
            web.loadHTMLString(html, baseURL: URL(fileURLWithPath: target).deletingLastPathComponent())
        }
    }

    func webView(_ w: WKWebView, didFinish nav: WKNavigation!) {
        if !js.isEmpty {
            w.evaluateJavaScript(js) { value, err in
                if let err = err { print("js error:", err.localizedDescription) }
                if !jsResultPath.isEmpty {
                    let text: String
                    if let s = value as? String { text = s }
                    else if let v = value { text = String(describing: v) }
                    else { text = "" }
                    try? text.write(toFile: jsResultPath, atomically: true, encoding: .utf8)
                }
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + waitSeconds) {
            if self.js2.isEmpty {
                self.capture()
            } else {
                // second phase: inspect page state, then snapshot
                w.evaluateJavaScript(self.js2) { value, err in
                    if let err = err { print("js2 error:", err.localizedDescription) }
                    if !jsResultPath.isEmpty {
                        let text = (value as? String) ?? (value.map { String(describing: $0) } ?? "")
                        try? text.write(toFile: jsResultPath, atomically: true, encoding: .utf8)
                    }
                    self.capture()
                }
            }
        }
    }

    func capture() {
        let cfg = WKPDFConfiguration()
        cfg.rect = web.bounds
        web.createPDF(configuration: cfg) { result in
            switch result {
            case .success(let data):
                let pdfPath = outPath + ".pdf"
                try? data.write(to: URL(fileURLWithPath: pdfPath))
                // rasterise with sips
                let p = Process()
                p.executableURL = URL(fileURLWithPath: "/usr/bin/sips")
                p.arguments = ["-s", "format", "png", pdfPath, "--out", outPath]
                p.standardOutput = FileHandle.nullDevice
                p.standardError = FileHandle.nullDevice
                try? p.run()
                p.waitUntilExit()
                // PDF page 1 is usually the whole visible area; also keep the
                // raw PDF around for inspection.
                if FileManager.default.fileExists(atPath: outPath) {
                    print("wrote \(outPath)")
                } else {
                    print("sips conversion failed; pdf at \(pdfPath)")
                }
            case .failure(let err):
                print("createPDF failed:", err.localizedDescription)
            }
            NSApp.terminate(nil)
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let shooter = Shooter()
shooter.start()
app.run()
