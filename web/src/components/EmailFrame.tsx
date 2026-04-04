/**
 * Sandboxed iframe for rendering sanitized email HTML.
 *
 * The HTML is already sanitized in Rust via ammonia before reaching the frontend.
 * The iframe uses `sandbox="allow-same-origin"` with no script execution.
 * Long messages scroll inside the iframe rather than expanding the detail pane.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 * @spec docs/L0-ui#html-email-rendering
 */
import { cn } from "../lib/utils";

/** @spec docs/L1-ui#messagedetail-and-emailframe */
interface EmailFrameProps {
  html: string;
  className?: string;
}

/**
 * Renders pre-sanitized email HTML inside a sandboxed `srcdoc` iframe.
 *
 * @spec docs/L1-ui#messagedetail-and-emailframe
 * @spec docs/L0-branding#color-palette-light-mode-primary
 */
export function EmailFrame({ html, className }: EmailFrameProps) {
  const wrappedHtml = `<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        :root { color-scheme: light; }
        body {
            font-family: "Geist", system-ui, sans-serif;
            font-size: 14px;
            line-height: 1.65;
            color: #141618;
            margin: 0;
            padding: 16px;
            background: #FFFFFF;
            word-wrap: break-word;
            overflow-wrap: break-word;
        }
        h1, h2, h3, h4 {
            color: #0D1117;
            line-height: 1.2;
        }
        img { max-width: 100%; height: auto; }
        a { color: #2B7EC2; }
        blockquote {
            border-left: 2px solid #D4DAE0;
            margin: 16px 0;
            padding: 4px 0 4px 16px;
            color: #5A6370;
        }
        pre {
            overflow-x: auto;
            background: #EEF1F4;
            padding: 12px 14px;
            border: 1px solid #D4DAE0;
        }
        code {
            font-family: "Geist Mono", monospace;
        }
    </style>
</head>
<body>${html}</body>
</html>`;

  return (
    <iframe
      className={cn("block h-full w-full border-0 bg-card", className)}
      sandbox="allow-same-origin"
      srcDoc={wrappedHtml}
      title="Email content"
    />
  );
}
