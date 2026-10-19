import { readdir } from "node:fs/promises";
import path from "node:path";
import { NextResponse } from "next/server";
import { dataPath } from "../_lib/data";

export const runtime = "nodejs";

async function stems(dir: string, extensions: string[]) {
  try {
    return (await readdir(dir))
      .filter((name) => !name.startsWith(".") && extensions.includes(path.extname(name).toLowerCase()))
      .map((name) => path.basename(name, path.extname(name)))
      .sort();
  } catch {
    return [];
  }
}

/** Names usable in an edit plan: sound effect types and overlay icons. */
export async function GET() {
  const [sounds, icons] = await Promise.all([
    stems(dataPath("se"), [".wav", ".mp3", ".m4a", ".ogg"]),
    stems(dataPath("icons"), [".png"]),
  ]);
  return NextResponse.json({ sounds, icons });
}
