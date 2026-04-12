import { readFile } from "node:fs/promises";
import path from "node:path";
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
};

export async function GET(_request: Request, context: RouteContext) {
  try {
    const { path: segments } = await context.params;
    const filename = safeName(segments.join("/"));
    const body = await readFile(path.join(dataPath("output"), filename));
    const contentType = contentTypes[path.extname(filename).toLowerCase()] ?? "application/octet-stream";

    return new NextResponse(body, {
      headers: {
        "Content-Type": contentType,
        "Cache-Control": "public, max-age=3600",
      },
    });
  } catch {
    return NextResponse.json({ error: "Output not found" }, { status: 404 });
  }
}
