import AppKit
import WebKit
import Foundation

// Native macOS WKWebView host for Threadline (`engram://app/threadline/index.html`).
// Uses WKURLSchemeHandler for GET asset/API routes and WKScriptMessageHandlerWithReply
// for POST/PUT mutations so request bodies are never dropped across WebKit XPC.

public typealias EngramNativeDispatchCallback = @convention(c) (
    UnsafeRawPointer?,
    UnsafePointer<CChar>?,
    UnsafePointer<CChar>?,
    UnsafePointer<CChar>?,
    Bool,
    UnsafePointer<CChar>?,
    UnsafePointer<CChar>?,
    UnsafePointer<CChar>?,
    UnsafeMutablePointer<UInt16>?,
    UnsafeMutablePointer<CChar>?,
    Int,
    UnsafeMutablePointer<UInt8>?,
    Int
) -> Int32

private final class NativeAppSchemeHandler: NSObject, WKURLSchemeHandler {
    let ctx: UnsafeRawPointer?
    let dispatch: EngramNativeDispatchCallback

    init(ctx: UnsafeRawPointer?, dispatch: @escaping EngramNativeDispatchCallback) {
        self.ctx = ctx
        self.dispatch = dispatch
    }

    func webView(_ webView: WKWebView, start urlSchemeTask: WKURLSchemeTask) {
        let req = urlSchemeTask.request
        guard let url = req.url else {
            urlSchemeTask.didFailWithError(NSError(domain: NSURLErrorDomain, code: NSURLErrorBadURL))
            return
        }
        let method = req.httpMethod ?? "GET"
        let scheme = url.scheme ?? "engram"
        let host = url.host ?? "app"
        let pathAndQuery = (url.path.isEmpty ? "/" : url.path) + (url.query.map { "?\($0)" } ?? "")
        let bodyStr = req.httpBody.flatMap { String(data: $0, encoding: .utf8) } ?? ""
        let localCtrl = req.value(forHTTPHeaderField: "X-Threadline-Local-Control") == "1"
        let origin = req.value(forHTTPHeaderField: "Origin") ?? "engram://app"

        var status: UInt16 = 500
        var cTypeBuf = [CChar](repeating: 0, count: 128)
        var bodyBuf = [UInt8](repeating: 0, count: 512 * 1024)

        let bodyLen = method.withCString { mPtr in
            pathAndQuery.withCString { tPtr in
                bodyStr.withCString { bPtr in
                    scheme.withCString { sPtr in
                        host.withCString { hPtr in
                            origin.withCString { oPtr in
                                dispatch(
                                    ctx, mPtr, tPtr, bPtr, localCtrl, sPtr, hPtr, oPtr,
                                    &status, &cTypeBuf, cTypeBuf.count, &bodyBuf, bodyBuf.count
                                )
                            }
                        }
                    }
                }
            }
        }

        let contentType = String(cString: cTypeBuf)
        let data = bodyLen > 0 ? Data(bytes: bodyBuf, count: Int(bodyLen)) : Data()
        let response = HTTPURLResponse(
            url: url,
            statusCode: Int(status),
            httpVersion: "HTTP/1.1",
            headerFields: [
                "Content-Type": contentType.isEmpty ? "application/octet-stream" : contentType,
                "Content-Length": "\(data.count)"
            ]
        )!
        urlSchemeTask.didReceive(response)
        urlSchemeTask.didReceive(data)
        urlSchemeTask.didFinish()
    }

    func webView(_ webView: WKWebView, stop urlSchemeTask: WKURLSchemeTask) {}
}

@_cdecl("engram_webview_is_linked")
public func engram_webview_is_linked() -> Bool {
    return true
}
