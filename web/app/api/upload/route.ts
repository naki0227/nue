import { randomUUID } from "node:crypto";
import { rename, writeFile } from "node:fs/promises";
import path from "node:path";
import { NextResponse } from "next/server";
import { dataPath, ensureDataDirs, safeName, writeJobStatus } from "../_lib/data";

export const runtime = "nodejs";

export async function POST(request: Request) {
  await ensureDataDirs();

  const formData = await request.formData();
  const file = formData.get("file");
  if (!(file instanceof File)) {
    return NextResponse.json({ error: "No file is received" }, { status: 400 });
  }

  const originalFilename = safeName(file.name);
  const extension = path.extname(originalFilename);
  const jobId = randomUUID();
  const storedFilename = `${jobId}${extension}`;
  const finalPath = path.join(dataPath("raw"), storedFilename);
  const tmpPath = `${finalPath}.part`;

  try {
    const bytes = Buffer.from(await file.arrayBuffer());
    await writeFile(tmpPath, bytes);

    const metadata = formData.get("metadata");
    if (typeof metadata === "string" && metadata.trim()) {
      let enrichedMetadata = metadata;
      try {
        const parsed = JSON.parse(metadata) as Record<string, unknown>;
        parsed.job_id = jobId;
        parsed.original_filename = originalFilename;
        enrichedMetadata = JSON.stringify(parsed, null, 2);
      } catch {
        // Keep the original metadata text. The analyzer can decide whether to ignore it.
      }
      await writeFile(`${finalPath}_metadata.json`, enrichedMetadata);
    }

    await writeJobStatus({
      job_id: jobId,
      status: "uploaded",
      original_filename: originalFilename,
      stored_filename: storedFilename,
      message: "Upload complete. Waiting for analysis.",
      progress: 10,
    });

    await rename(tmpPath, finalPath);
  } catch (error) {
    await writeJobStatus({
      job_id: jobId,
      status: "failed",
      original_filename: originalFilename,
      stored_filename: storedFilename,
      message: error instanceof Error ? error.message : "Upload could not be finalized.",
      progress: 100,
    }).catch(() => undefined);

    return NextResponse.json(
      { error: error instanceof Error ? error.message : "Unable to save file" },
      { status: 500 },
    );
  }

  return NextResponse.json(
    {
      message: "File uploaded successfully",
      job_id: jobId,
      filename: storedFilename,
      status: "processing_started",
    },
    { status: 202 },
  );
}
