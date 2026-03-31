import { useRef, useEffect, useState } from "react";

interface EmailFrameProps {
  html: string;
}

export function EmailFrame({ html }: EmailFrameProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(300);

  const wrappedHtml = `<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        :root {
            color-scheme: light;
        }
        body {
            font-family: "Iowan Old Style", "Palatino Linotype", "Book Antiqua", serif;
            font-size: 17px;
            line-height: 1.72;
            color: #2f2b28;
            margin: 0;
            padding: 24px;
            background: #fcfbf8;
            word-wrap: break-word;
            overflow-wrap: break-word;
        }
        h1, h2, h3, h4 {
            font-family: "Iowan Old Style", "Palatino Linotype", serif;
            color: #25211d;
            line-height: 1.2;
        }
        img { max-width: 100%; height: auto; }
        a { color: #355d83; }
        blockquote {
            border-left: 2px solid #c8c0b0;
            margin: 16px 0;
            padding: 4px 0 4px 16px;
            color: #6f6a61;
        }
        pre {
            overflow-x: auto;
            background: #f2efe8;
            padding: 12px 14px;
            border-radius: 8px;
        }
        code {
            font-family: "SFMono-Regular", Consolas, monospace;
        }
    </style>
</head>
<body>${html}</body>
</html>`;

  useEffect(() => {
    const iframe = iframeRef.current;
    if (!iframe) return;

    const onLoad = () => {
      try {
        const doc = iframe.contentDocument;
        if (doc?.body) {
          setHeight(doc.body.scrollHeight + 32);
        }
      } catch {
        // Cross-origin access denied — use default height
      }
    };

    iframe.addEventListener("load", onLoad);
    return () => iframe.removeEventListener("load", onLoad);
  }, [html]);

  return (
    <iframe
      ref={iframeRef}
      sandbox="allow-same-origin"
      srcDoc={wrappedHtml}
      title="Email content"
      style={{
        width: "100%",
        height: `${height}px`,
        border: "none",
        display: "block",
      }}
    />
  );
}
