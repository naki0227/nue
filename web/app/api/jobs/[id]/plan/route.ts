import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import path from "node:path";
import { NextResponse } from "next/server";
import { dataPath, readJobStatus, safeJobId, writeJobStatus } from "../../../_lib/data";

export const runtime = "nodejs";

interface RouteContext {
  params: Promise<{
    id: string;
  }>;
}

async function readPlan(jobId: string) {
  const candidates = [
    path.join(dataPath("json"), `${jobId}.json`),
    path.join(dataPath("output"), `${jobId}_plan.json`),
  ];
  for (const candidate of candidates) {
    try {
      return JSON.parse(await readFile(candidate, "utf8")) as Record<string, unknown>;
    } catch {
      // try the next location
    }
  }
  return null;
}

export async function GET(_request: Request, context: RouteContext) {
  try {
    const jobId = safeJobId((await context.params).id);
    const plan = await readPlan(jobId);
    return plan
      ? NextResponse.json(plan)
      : NextResponse.json({ error: "Plan not found" }, { status: 404 });
  } catch {
    return NextResponse.json({ error: "Invalid job id" }, { status: 400 });
  }
}

/** Queues a hand-edited plan for the Rust worker to re-render. */
export async function PUT(request: Request, context: RouteContext) {
  let jobId: string;
  try {
    jobId = safeJobId((await context.params).id);
  } catch {
    return NextResponse.json({ error: "Invalid job id" }, { status: 400 });
  }

  const current = await readPlan(jobId);
  if (!current) {
    return NextResponse.json({ error: "Plan not found" }, { status: 404 });
  }

  let edited: Record<string, unknown>;
  try {
    edited = (await request.json()) as Record<string, unknown>;
  } catch {
    return NextResponse.json({ error: "Body must be JSON" }, { status: 400 });
  }
  if (!Array.isArray(edited.cuts) || edited.cuts.length === 0) {
    return NextResponse.json({ error: "Keep at least one cut" }, { status: 400 });
  }

  // The source video and job identity always come from the server copy.
  const plan = { ...edited, job_id: jobId, original_filename: current.original_filename };
  const editsDir = dataPath("edits");
  await mkdir(editsDir, { recursive: true });
  const tmpPath = path.join(editsDir, `.${jobId}.json.tmp`);
  await writeFile(tmpPath, JSON.stringify(plan, null, 2));

  const previous = await readJobStatus(jobId).catch(() => null);
  await writeJobStatus({
    ...(previous ?? {}),
    job_id: jobId,
    status: "rendering",
    message: "Re-render queued.",
    progress: 65,
  });
  await rename(tmpPath, path.join(editsDir, `${jobId}.json`));

  return NextResponse.json({ status: "queued" }, { status: 202 });
}
