import { NextResponse } from "next/server";
import { readJobStatus } from "../../_lib/data";

export const runtime = "nodejs";

interface RouteContext {
  params: Promise<{
    id: string;
  }>;
}

export async function GET(_request: Request, context: RouteContext) {
  try {
    const { id } = await context.params;
    return NextResponse.json(await readJobStatus(id));
  } catch (error) {
    const status = error instanceof Error && error.message === "Invalid job id" ? 400 : 404;
    return NextResponse.json({ error: "Job not found" }, { status });
  }
}
