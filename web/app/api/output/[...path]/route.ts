import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import path from "node:path";
import { Readable } from "node:stream";
import { NextResponse } from "next/server";
import { dataPath, safeName } from "../../_lib/data";

export const runtime = "nodejs";

interface RouteContext {
  params: Promise<{
    path: string[];
  }>;
}

const contentTypes: Record<string, string> = {
  ".mp4": "video/mp4",
  ".mov": "video/quicktime",
  ".webm": "video/webm",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".png": "image/png",
  ".srt": "application/x-subrip; charset=utf-8",
  ".json": "application/json; charset=utf-8",
};

function stream(filePath: string, start?: number, end?: number) {
  return Readable.toWeb(createReadStream(filePath, { start, end })) as ReadableStream;
}

// Supports HTTP Range requests: Safari (and iOS) will not play video without them.
export async function GET(request: Request, context: RouteContext) {
  let filePath: string;
  let size: number;
  try {
    const { path: segments } = await context.params;
    filePath = path.join(dataPath("output"), safeName(segments.join("/")));
    size = (await stat(filePath)).size;
  } catch {
    return NextResponse.json({ error: "Output not found" }, { status: 404 });
  }

  const extension = path.extname(filePath).toLowerCase();
  const headers: Record<string, string> = {
    "Content-Type": contentTypes[extension] ?? "application/octet-stream",
    "Accept-Ranges": "bytes",
    "Cache-Control": "no-cache",
  };
  if (extension === ".srt" || extension === ".json") {
    headers["Content-Disposition"] = `attachment; filename="${path.basename(filePath)}"`;
  }

  const range = /^bytes=(\d*)-(\d*)$/.exec(request.headers.get("range") ?? "");
  if (range && (range[1] || range[2])) {
    const start = range[1] ? Number(range[1]) : Math.max(size - Number(range[2]), 0);
    const end = range[1] && range[2] ? Math.min(Number(range[2]), size - 1) : size - 1;
    if (start >= size || start > end) {
      return new NextResponse(null, { status: 416, headers: { "Content-Range": `bytes */${size}` } });
    }
    return new NextResponse(stream(filePath, start, end), {
      status: 206,
      headers: {
        ...headers,
        "Content-Range": `bytes ${start}-${end}/${size}`,
        "Content-Length": String(end - start + 1),
      },
    });
  }

  return new NextResponse(stream(filePath), {
    headers: { ...headers, "Content-Length": String(size) },
  });
}
