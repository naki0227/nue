import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

export interface JobStatus {
  job_id: string;
  status: "uploaded" | "analyzing" | "rendering" | "completed" | "failed";
  original_filename?: string;
  stored_filename?: string;
  message?: string;
  progress?: number;
  updated_at?: string;
  output_url?: string;
  thumbnail_url?: string;
}

export function dataDir() {
  if (process.env.NUE_DATA_DIR) {
    return process.env.NUE_DATA_DIR;
  }
  if (process.env.DATA_DIR) {
    return process.env.DATA_DIR;
  }
  if (existsSync("/app/data")) {
    return "/app/data";
  }
  if (process.env.NODE_ENV !== "production") {
    return path.join(process.cwd(), "..", "data");
  }
  return path.join(process.cwd(), "data");
}

export function dataPath(name: "raw" | "jobs" | "output") {
  return path.join(dataDir(), name);
}

export async function ensureDataDirs() {
  await Promise.all([
    mkdir(dataPath("raw"), { recursive: true }),
    mkdir(dataPath("jobs"), { recursive: true }),
    mkdir(dataPath("output"), { recursive: true }),
  ]);
}

export function safeName(value: string) {
  const name = path.basename(value);
  if (
    !name ||
    name !== value ||
    name === "." ||
    name.includes("..") ||
    name.includes("/") ||
    name.includes("\\")
  ) {
    throw new Error("Invalid file name");
  }
  return name;
}

export function safeJobId(value: string) {
  if (!/^[a-zA-Z0-9._-]+$/.test(value) || value === "." || value.includes("..")) {
    throw new Error("Invalid job id");
  }
  return value;
}

export async function writeJobStatus(status: JobStatus) {
  await mkdir(dataPath("jobs"), { recursive: true });
  const payload = {
    ...status,
    updated_at: new Date().toISOString(),
  };
  await writeFile(
    path.join(dataPath("jobs"), `${status.job_id}.json`),
    JSON.stringify(payload, null, 2),
  );
}

export async function readJobStatus(jobId: string) {
  const safeId = safeJobId(jobId);
  const body = await readFile(path.join(dataPath("jobs"), `${safeId}.json`), "utf8");
  return JSON.parse(body) as JobStatus;
}
