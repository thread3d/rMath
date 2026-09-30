// Screenshot and bug-report support, split out of main.swift.
//
// An `extension AppDelegate`, so the app keeps a single delegate type while
// the reporting code stays out of the middle of the window and server code.
import Cocoa
import WebKit

extension AppDelegate {
    // ---- screenshots

    func startShotWatcher() {
        try? FileManager.default.removeItem(at: shotRequestURL())
        try? FileManager.default.removeItem(at: shotDoneURL())
        shotTimer = Timer.scheduledTimer(withTimeInterval: 0.4, repeats: true) { [weak self] _ in
            self?.pollShotRequest()
        }
    }

    /// Request format (a text file the running app polls):
    ///   line 1: destination path ("" = default Desktop path)
    ///   line 2: optional JavaScript to run first (e.g. evaluate an expression)
    ///   line 3: optional delay in seconds before the snapshot (default 2.5)
    ///   line 4: optional target — "page" (default) or "palette:<name>"
    func pollShotRequest() {
        let req = shotRequestURL()
        guard let raw = try? String(contentsOf: req, encoding: .utf8) else { return }
        try? FileManager.default.removeItem(at: req)
        var lines = raw.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        let dest = lines.isEmpty ? "" : lines.removeFirst().trimmingCharacters(in: .whitespaces)
        let js = lines.isEmpty ? "" : lines.removeFirst().trimmingCharacters(in: .whitespacesAndNewlines)
        let delay = lines.isEmpty
            ? 2.5
            : (Double(lines.removeFirst().trimmingCharacters(in: .whitespaces)) ?? 2.5)
        let what = lines.isEmpty ? "page" : lines.removeFirst().trimmingCharacters(in: .whitespaces)
        let target = dest.isEmpty ? defaultShotPath() : dest

        if what == "report" {
            generateBugReport(note: "triggered from the command line") { path in
                self.finishShot(path.map { "ok \($0)" } ?? "err report failed")
            }
            return
        }

        if what.hasPrefix("palette:") {
            let name = String(what.dropFirst("palette:".count))
            showPalette(named: name)
            DispatchQueue.main.asyncAfter(deadline: .now() + max(delay, 1.0)) {
                self.savePaletteScreenshot(named: name, to: target)
            }
            return
        }
        guard !js.isEmpty, let web = webView else {
            saveScreenshot(to: target)
            return
        }
        web.evaluateJavaScript(js) { value, err in
            if let err = err { print("js error: \(err.localizedDescription)") }
            // Hand the script's string result back on disk: without it a --js
            // phase can only be observed through the pixels it paints.
            if let text = value as? String {
                let out = URL(fileURLWithPath: NSTemporaryDirectory())
                    .appendingPathComponent("rMath-shot.result")
                try? text.write(to: out, atomically: true, encoding: .utf8)
                print("js result: \(out.path)")
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) {
                self.saveScreenshot(to: target)
            }
        }
    }

    /// Snapshot a palette window by rendering its layer tree (works for an
    /// on-screen window and needs no screen-recording permission).
    func savePaletteScreenshot(named name: String, to path: String) {
        guard let win = palettes[name], let view = win.contentView else {
            finishShot("err no palette \(name)")
            return
        }
        let dir = URL(fileURLWithPath: path).deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        view.wantsLayer = true
        guard let layer = view.layer else {
            finishShot("err palette has no layer")
            return
        }
        let w = Int(view.bounds.width), h = Int(view.bounds.height)
        guard w > 0, h > 0,
              let rep = NSBitmapImageRep(
                bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h,
                bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0) else {
            finishShot("err palette bitmap")
            return
        }
        rep.size = NSSize(width: view.bounds.width, height: view.bounds.height)
        NSGraphicsContext.saveGraphicsState()
        if let ctx = NSGraphicsContext(bitmapImageRep: rep) {
            NSGraphicsContext.current = ctx
            layer.render(in: ctx.cgContext)
        }
        NSGraphicsContext.restoreGraphicsState()
        if let png = rep.representation(using: .png, properties: [:]) {
            try? png.write(to: URL(fileURLWithPath: path))
            finishShot("ok \(path)")
        } else {
            finishShot("err png encode")
        }
    }

    func defaultShotPath() -> String {
        let name = "rMath-\(timestamp()).png"
        if let desktop = FileManager.default.urls(for: .desktopDirectory,
                                                  in: .userDomainMask).first {
            return desktop.appendingPathComponent(name).path
        }
        return URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent(name).path
    }

    /// Snapshot the page to PNG data (no side effects).
    func snapshotPNG(completion: @escaping (Data?) -> Void) {
        guard let web = webView else { completion(nil); return }
        let cfg = WKSnapshotConfiguration()
        cfg.rect = web.bounds
        web.takeSnapshot(with: cfg) { image, _ in
            guard let image = image,
                  let tiff = image.tiffRepresentation,
                  let rep = NSBitmapImageRep(data: tiff) else { completion(nil); return }
            completion(rep.representation(using: .png, properties: [:]))
        }
    }

    /// Snapshot the web view. `.pdf` destinations produce a full page PDF
    /// instead of a viewport PNG. Needs no screen-recording permission: the
    /// app renders its own view.
    func saveScreenshot(to path: String) {
        // Create the destination directory if needed.
        let dir = URL(fileURLWithPath: path).deletingLastPathComponent()
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        guard let web = webView else {
            finishShot("err no window")
            return
        }
        if path.lowercased().hasSuffix(".pdf") {
            // Ask the page for its full height so the PDF covers all the
            // content rather than just the visible viewport.
            web.evaluateJavaScript(
                "Math.max(document.body.scrollHeight, document.documentElement.scrollHeight)"
            ) { value, _ in
                var height = web.bounds.height
                if let n = value as? NSNumber, n.doubleValue > 0 {
                    height = CGFloat(n.doubleValue)
                }
                let cfg = WKPDFConfiguration()
                cfg.rect = CGRect(x: 0, y: 0, width: web.bounds.width, height: height)
                web.createPDF(configuration: cfg) { result in
                    switch result {
                    case .success(let data):
                        do {
                            try data.write(to: URL(fileURLWithPath: path))
                            self.finishShot("ok \(path)")
                        } catch { self.finishShot("err \(error.localizedDescription)") }
                    case .failure(let err):
                        self.finishShot("err \(err.localizedDescription)")
                    }
                }
            }
            return
        }
        let cfg = WKSnapshotConfiguration()
        cfg.rect = web.bounds
        web.takeSnapshot(with: cfg) { image, err in
            guard let image = image,
                  let tiff = image.tiffRepresentation,
                  let rep = NSBitmapImageRep(data: tiff),
                  let png = rep.representation(using: .png, properties: [:]) else {
                self.finishShot("err \(err?.localizedDescription ?? "snapshot failed")")
                return
            }
            do {
                try png.write(to: URL(fileURLWithPath: path))
                self.finishShot("ok \(path)")
            } catch {
                self.finishShot("err \(error.localizedDescription)")
            }
        }
    }

    func finishShot(_ message: String) {
        try? message.write(to: shotDoneURL(), atomically: true, encoding: .utf8)
        print("screenshot: \(message)")
        if message.hasPrefix("ok ") {
            let path = String(message.dropFirst(3))
            NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
        }
    }

    /// Build a self-contained bug report: screenshot, page transcript, the
    /// last error, versions, and the tail of the log. The path and a one-line
    /// summary go on the clipboard, ready to paste into a chat.
    func generateBugReport(note: String = "", completion: ((String?) -> Void)? = nil) {
        let stamp = timestamp()
        let dir = reportDir()
        let png = dir.appendingPathComponent("bug-\(stamp).png")
        let md = dir.appendingPathComponent("bug-\(stamp).md")

        runJSCollect("reportContext()") { ctx in
            self.snapshotPNG { pngData in
                if let pngData = pngData {
                    try? pngData.write(to: png)
                }
                let body = self.buildReportBody(stamp: stamp, png: png.path,
                                                context: ctx, note: note)
                try? body.write(to: md, atomically: true, encoding: .utf8)

                // Paste-ready text on the clipboard.
                var paste = "rMath bug report: \(md.path)"
                if pngData != nil { paste += "\nscreenshot: \(png.path)" }
                if let summary = self.reportSummary(ctx) { paste += "\n" + summary }
                if !note.isEmpty { paste += "\nnote: " + note }
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(paste, forType: .string)

                self.runJS("showReport('\(md.path.replacingOccurrences(of: "'", with: "\\'"))', '\(self.reportSummary(ctx)?.replacingOccurrences(of: "'", with: "\\'") ?? "")')")
                completion?(md.path)
            }
        }
    }

    func reportSummary(_ ctx: String?) -> String? {
        guard let ctx = ctx, let data = ctx.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return nil }
        guard let last = obj["lastError"] as? [String: Any] else { return "no errors in this session" }
        let input = (last["input"] as? String ?? "").prefix(120)
        let msg = ((last["error"] as? String ?? "") + " " + (last["messages"] as? String ?? ""))
            .trimmingCharacters(in: .whitespacesAndNewlines).prefix(160)
        return "last error: \(input)\(msg.isEmpty ? "" : " — " + msg)"
    }

    func buildReportBody(stamp: String, png: String, context: String?, note: String) -> String {
        var out = "# rMath bug report — \(stamp)\n\n"
        out += "- app: rMath \(Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "?")\n"
        out += "- bundle: \(Bundle.main.bundlePath)\n"
        out += "- kernel: \(kernelVersion())\n"
        out += "- python: \(pythonVersion())\n"
        out += "- port: \(port)\n"
        out += "- log: \(logURL().path)\n"
        out += "- screenshot: \(png)\n"
        if !note.isEmpty { out += "- note: \(note)\n" }
        out += "\n## Page context\n\n```json\n\(context ?? "(unavailable)")\n```\n"
        out += "\n## Log tail\n\n```\n\(logTail(40))\n```\n"
        return out
    }

    /// Run JS and pass its string result back on the main thread.
    func runJSCollect(_ js: String, completion: @escaping (String?) -> Void) {
        webView?.evaluateJavaScript(js) { value, err in
            if let err = err { print("js collect error: \(err.localizedDescription)") }
            completion(value as? String)
        }
    }

    func kernelVersion() -> String {
        let bin = resourcesDir().appendingPathComponent("rmath")
        let p = Process()
        p.executableURL = bin
        p.arguments = ["--version"]
        let pipe = Pipe()
        p.standardOutput = pipe
        try? p.run()
        p.waitUntilExit()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        return String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines)
            ?? "unknown"
    }

    func pythonVersion() -> String {
        guard let py = pythonCandidates().first(where: usablePython) else { return "not found" }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: py)
        p.arguments = ["-c", "import sys; print(sys.executable, sys.version.split()[0])"]
        let pipe = Pipe()
        p.standardOutput = pipe
        try? p.run()
        p.waitUntilExit()
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        let text = String(data: data, encoding: .utf8)?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? "?"
        return "\(text)  [\(py)]"
    }

    func logTail(_ n: Int) -> String {
        guard let text = try? String(contentsOf: logURL(), encoding: .utf8) else {
            return "(no log)"
        }
        let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
        return lines.suffix(n).joined(separator: "\n")
    }

    @objc func menuReportBug() {
        generateBugReport { path in
            if let path = path { print("report: \(path)") }
        }
    }

    @objc func shootToDesktop() {
        saveScreenshot(to: defaultShotPath())
    }

    /// Ask the running instance to generate a bug report.
    func requestReport() -> Bool {
        if !healthOK(port) {
            FileHandle.standardError.write(Data(
                "rMath: no running instance on port \(port)\n".utf8))
            return false
        }
        try? FileManager.default.removeItem(at: shotDoneURL())
        let request = "\n\n0.5\nreport\n"
        do {
            try request.write(to: shotRequestURL(), atomically: true, encoding: .utf8)
        } catch { return false }
        let deadline = Date().addingTimeInterval(180)
        while Date() < deadline {
            if let done = try? String(contentsOf: shotDoneURL(), encoding: .utf8) {
                try? FileManager.default.removeItem(at: shotDoneURL())
                print(done)
                return done.hasPrefix("ok ")
            }
            usleep(150_000)
        }
        return false
    }

    /// Ask the running instance for a screenshot; returns true on success.
    func requestScreenshot(dest: String, js: String = "", delay: Double = 2.5,
                           what: String = "page") -> Bool {
        if !healthOK(port) {
            FileHandle.standardError.write(Data(
                "rMath: no running instance on port \(port) to screenshot\n".utf8))
            return false
        }
        try? FileManager.default.removeItem(at: shotDoneURL())
        let target = dest.isEmpty ? URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("rMath-\(timestamp()).png").path : dest
        let request = "\(target)\n\(js.replacingOccurrences(of: "\n", with: " "))\n\(delay)\n\(what)\n"
        do {
            try request.write(to: shotRequestURL(), atomically: true, encoding: .utf8)
        } catch {
            FileHandle.standardError.write(Data("rMath: cannot write request: \(error)\n".utf8))
            return false
        }
        // Generous: the page may be busy evaluating a long file when the
        // request arrives.
        let deadline = Date().addingTimeInterval(180)
        while Date() < deadline {
            if let done = try? String(contentsOf: shotDoneURL(), encoding: .utf8) {
                try? FileManager.default.removeItem(at: shotDoneURL())
                print(done)
                return done.hasPrefix("ok ")
            }
            usleep(150_000)
        }
        FileHandle.standardError.write(Data("rMath: screenshot timed out\n".utf8))
        return false
    }
}
