"use client";

import { useEffect, useMemo, useState } from "react";
import { Loader2, Plus, RefreshCw, Scissors, Trash2, X } from "lucide-react";

// Mirrors docs/edit-plan.schema.json (only the fields the editor touches are typed).
interface Style {
  color?: string;
  position?: string;
  box?: boolean;
}
interface Cut {
  start_time: string;
  end_time: string;
  [key: string]: unknown;
}
interface TimedText {
  start: string;
  end: string;
  text: string;
  style?: Style;
}
interface SeEvent {
  timestamp: string;
  type: string;
  tag?: string | null;
}
interface VisualEffect {
  start: string;
  end?: string;
  type: string;
}
interface Overlay {
  asset: string;
  start: string;
  end: string;
  position?: string;
  scale?: number;
}
export interface EditPlan {
  original_filename: string;
  format?: string | null;
  cuts: Cut[];
  telops?: TimedText[] | null;
  subtitles?: TimedText[] | null;
  se_events?: SeEvent[] | null;
  visual_effects?: VisualEffect[] | null;
  overlays?: Overlay[] | null;
  options?: { bgm?: boolean; sound_effects?: boolean; subtitles?: boolean };
  [key: string]: unknown;
}

export function parseTime(value: string): number {
  const parts = value.trim().replace(",", ".").split(":").map(Number);
  if (parts.some((part) => Number.isNaN(part))) return NaN;
  return parts.reduce((total, part) => total * 60 + part, 0);
}

export function formatTime(seconds: number): string {
  const ms = Math.round(Math.max(seconds, 0) * 1000);
  const pad = (n: number, width = 2) => String(n).padStart(width, "0");
  return `${pad(Math.floor(ms / 3_600_000))}:${pad(Math.floor((ms % 3_600_000) / 60_000))}:${pad(
    Math.floor((ms % 60_000) / 1000),
  )}.${pad(ms % 1000, 3)}`;
}

const inputClass = "px-2 py-1.5 bg-black/50 border border-white/10 rounded-lg text-xs min-w-0";
const COLORS = ["yellow", "white", "cyan", "pink", "red", "green", "orange"];
const EFFECTS = ["zoom_in", "zoom_out", "pan_left", "pan_right"];
const POSITIONS = ["top_right", "top_left", "bottom_right", "bottom_left", "center"];

function TimeInput({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const invalid = Number.isNaN(parseTime(value));
  return (
    <input
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className={`${inputClass} w-[7.5rem] font-mono ${invalid ? "border-red-500" : ""}`}
    />
  );
}

function Section({ title, onAdd, children }: { title: string; onAdd?: () => void; children: React.ReactNode }) {
  return (
    <section className="space-y-2">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-gray-300">{title}</h3>
        {onAdd && (
          <button onClick={onAdd} className="px-3 py-1 bg-purple-500/20 hover:bg-purple-500/30 rounded-lg text-xs flex items-center gap-1">
            <Plus className="w-3 h-3" /> Add
          </button>
        )}
      </div>
      {children}
    </section>
  );
}

function RemoveButton({ onClick }: { onClick: () => void }) {
  return (
    <button onClick={onClick} aria-label="Remove" className="p-1.5 bg-red-500/20 hover:bg-red-500/30 rounded-lg shrink-0">
      <X className="w-3.5 h-3.5" />
    </button>
  );
}

interface Props {
  jobId: string;
  planUrl: string;
  onQueued: () => void;
}

export default function PlanEditor({ jobId, planUrl, onQueued }: Props) {
  const [plan, setPlan] = useState<EditPlan | null>(null);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const [assets, setAssets] = useState<{ sounds: string[]; icons: string[] }>({ sounds: [], icons: [] });

  useEffect(() => {
    fetch(`/api/jobs/${jobId}/plan`)
      .then((r) => (r.ok ? r.json() : fetch(planUrl).then((fallback) => fallback.json())))
      .then((data: EditPlan) => setPlan(data))
      .catch(() => setError("Could not load the edit plan."));
    fetch("/api/assets")
      .then((r) => r.json())
      .then(setAssets)
      .catch(() => undefined);
  }, [jobId, planUrl]);

  const timelineEnd = useMemo(() => {
    if (!plan) return 1;
    const ends = [
      ...plan.cuts.map((c) => parseTime(c.end_time)),
      ...(plan.subtitles ?? []).map((s) => parseTime(s.end)),
    ].filter((t) => !Number.isNaN(t));
    return Math.max(1, ...ends);
  }, [plan]);

  if (error) return <p className="text-sm text-red-400">{error}</p>;
  if (!plan) return <Loader2 className="w-5 h-5 animate-spin mx-auto text-gray-400" />;

  const update = (patch: Partial<EditPlan>) => setPlan({ ...plan, ...patch });
  const list = <K extends "telops" | "subtitles" | "se_events" | "visual_effects" | "overlays">(key: K) =>
    (plan[key] ?? []) as NonNullable<EditPlan[K]>;
  function setItem<T>(key: "telops" | "subtitles" | "se_events" | "visual_effects" | "overlays", index: number, item: T) {
    const next = [...(list(key) as T[])];
    next[index] = item;
    update({ [key]: next } as Partial<EditPlan>);
  }
  function removeItem(key: "telops" | "subtitles" | "se_events" | "visual_effects" | "overlays", index: number) {
    update({ [key]: (list(key) as unknown[]).filter((_, i) => i !== index) } as Partial<EditPlan>);
  }
  const setCut = (index: number, cut: Cut) => update({ cuts: plan.cuts.map((c, i) => (i === index ? cut : c)) });
  const splitCut = (index: number) => {
    const cut = plan.cuts[index];
    const start = parseTime(cut.start_time);
    const end = parseTime(cut.end_time);
    if (!(end - start > 1)) return;
    const middle = formatTime((start + end) / 2);
    const cuts = [...plan.cuts];
    cuts.splice(index, 1, { ...cut, end_time: middle }, { ...cut, start_time: middle });
    update({ cuts });
  };

  const invalid =
    plan.cuts.length === 0 ||
    plan.cuts.some((c) => !(parseTime(c.end_time) > parseTime(c.start_time)));

  const save = async () => {
    setSaving(true);
    setError("");
    try {
      const response = await fetch(`/api/jobs/${jobId}/plan`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(plan),
      });
      if (!response.ok) {
        const body = (await response.json().catch(() => ({}))) as { error?: string };
        throw new Error(body.error ?? "Re-render failed to start.");
      }
      onQueued();
    } catch (e) {
      setError(e instanceof Error ? e.message : "Re-render failed to start.");
      setSaving(false);
    }
  };

  const options = plan.options ?? {};
  const setOption = (key: "bgm" | "sound_effects" | "subtitles", value: boolean) =>
    update({ options: { ...options, [key]: value } });
  const now = formatTime(0);

  return (
    <div className="text-left space-y-6" onClick={(e) => e.stopPropagation()}>
      {/* Timeline: kept cuts over the source, with telops, SE and icons as markers. */}
      <div>
        <div className="relative h-10 rounded-lg bg-white/5 overflow-hidden">
          {plan.cuts.map((cut, i) => {
            const s = parseTime(cut.start_time);
            const e = parseTime(cut.end_time);
            if (Number.isNaN(s) || Number.isNaN(e)) return null;
            return (
              <div
                key={i}
                title={`Cut ${i + 1}: ${cut.start_time} - ${cut.end_time}`}
                className="absolute top-1 bottom-1 rounded bg-purple-500/60 border border-purple-300/60"
                style={{ left: `${(s / timelineEnd) * 100}%`, width: `${((e - s) / timelineEnd) * 100}%` }}
              />
            );
          })}
          {list("telops").map((t, i) => (
            <div key={`t${i}`} className="absolute top-0 h-1.5 bg-yellow-300" style={{ left: `${(parseTime(t.start) / timelineEnd) * 100}%`, width: `${(Math.max(parseTime(t.end) - parseTime(t.start), 0.2) / timelineEnd) * 100}%` }} />
          ))}
          {list("se_events").map((se, i) => (
            <div key={`s${i}`} title={se.type} className="absolute bottom-0 w-1 h-3 bg-cyan-300" style={{ left: `${(parseTime(se.timestamp) / timelineEnd) * 100}%` }} />
          ))}
        </div>
        <div className="flex justify-between text-[10px] text-gray-500 font-mono mt-1">
          <span>00:00</span>
          <span>purple = kept · yellow = telop · cyan = sound</span>
          <span>{formatTime(timelineEnd).slice(3, 8)}</span>
        </div>
      </div>

      <Section title={`Cuts (${plan.cuts.length})`}>
        {plan.cuts.map((cut, i) => (
          <div key={i} className="flex gap-2 items-center flex-wrap">
            <span className="text-xs text-gray-500 w-5">{i + 1}</span>
            <TimeInput value={cut.start_time} onChange={(v) => setCut(i, { ...cut, start_time: v })} />
            <span className="text-gray-500">–</span>
            <TimeInput value={cut.end_time} onChange={(v) => setCut(i, { ...cut, end_time: v })} />
            <button onClick={() => splitCut(i)} aria-label="Split" title="Split in half" className="p-1.5 bg-white/10 hover:bg-white/20 rounded-lg">
              <Scissors className="w-3.5 h-3.5" />
            </button>
            {plan.cuts.length > 1 && <RemoveButton onClick={() => update({ cuts: plan.cuts.filter((_, j) => j !== i) })} />}
          </div>
        ))}
      </Section>

      <Section
        title="Telops"
        onAdd={() => update({ telops: [...list("telops"), { start: now, end: formatTime(3), text: "", style: { color: "yellow", position: "top", box: true } }] })}
      >
        {list("telops").map((t, i) => (
          <div key={i} className="flex gap-2 items-center flex-wrap">
            <TimeInput value={t.start} onChange={(v) => setItem("telops", i, { ...t, start: v })} />
            <TimeInput value={t.end} onChange={(v) => setItem("telops", i, { ...t, end: v })} />
            <input value={t.text} onChange={(e) => setItem("telops", i, { ...t, text: e.target.value })} placeholder="Text" className={`${inputClass} flex-1 min-w-[8rem]`} />
            <select value={t.style?.color ?? "yellow"} onChange={(e) => setItem("telops", i, { ...t, style: { ...t.style, color: e.target.value } })} className={inputClass}>
              {COLORS.map((c) => <option key={c}>{c}</option>)}
            </select>
            <RemoveButton onClick={() => removeItem("telops", i)} />
          </div>
        ))}
      </Section>

      <Section title={`Subtitles (${list("subtitles").length})`}>
        <div className="max-h-64 overflow-y-auto space-y-2 pr-1">
          {list("subtitles").map((t, i) => (
            <div key={i} className="flex gap-2 items-center">
              <span className="text-[10px] font-mono text-gray-500 w-14 shrink-0">{t.start.slice(3, 8)}</span>
              <input value={t.text} onChange={(e) => setItem("subtitles", i, { ...t, text: e.target.value })} className={`${inputClass} flex-1`} />
              <RemoveButton onClick={() => removeItem("subtitles", i)} />
            </div>
          ))}
        </div>
      </Section>

      <Section title="Sound effects" onAdd={() => update({ se_events: [...list("se_events"), { timestamp: now, type: assets.sounds[0] ?? "impact" }] })}>
        {list("se_events").map((se, i) => (
          <div key={i} className="flex gap-2 items-center">
            <TimeInput value={se.timestamp} onChange={(v) => setItem("se_events", i, { ...se, timestamp: v })} />
            <select value={se.type} onChange={(e) => setItem("se_events", i, { ...se, type: e.target.value })} className={`${inputClass} flex-1`}>
              {[...new Set([se.type, ...assets.sounds])].map((name) => <option key={name}>{name}</option>)}
            </select>
            <RemoveButton onClick={() => removeItem("se_events", i)} />
          </div>
        ))}
      </Section>

      <Section title="Zoom / pan" onAdd={() => update({ visual_effects: [...list("visual_effects"), { start: now, end: formatTime(1.5), type: "zoom_in" }] })}>
        {list("visual_effects").map((fx, i) => (
          <div key={i} className="flex gap-2 items-center flex-wrap">
            <TimeInput value={fx.start} onChange={(v) => setItem("visual_effects", i, { ...fx, start: v })} />
            <TimeInput value={fx.end ?? ""} onChange={(v) => setItem("visual_effects", i, { ...fx, end: v })} />
            <select value={fx.type} onChange={(e) => setItem("visual_effects", i, { ...fx, type: e.target.value })} className={`${inputClass} flex-1`}>
              {EFFECTS.map((name) => <option key={name}>{name}</option>)}
            </select>
            <RemoveButton onClick={() => removeItem("visual_effects", i)} />
          </div>
        ))}
      </Section>

      <Section title="Icons" onAdd={() => update({ overlays: [...list("overlays"), { asset: assets.icons[0] ?? "exclaim", start: now, end: formatTime(1), position: "top_right", scale: 0.18 }] })}>
        {list("overlays").map((o, i) => (
          <div key={i} className="flex gap-2 items-center flex-wrap">
            <TimeInput value={o.start} onChange={(v) => setItem("overlays", i, { ...o, start: v })} />
            <TimeInput value={o.end} onChange={(v) => setItem("overlays", i, { ...o, end: v })} />
            <select value={o.asset} onChange={(e) => setItem("overlays", i, { ...o, asset: e.target.value })} className={inputClass}>
              {[...new Set([o.asset, ...assets.icons])].map((name) => <option key={name}>{name}</option>)}
            </select>
            <select value={o.position ?? "top_right"} onChange={(e) => setItem("overlays", i, { ...o, position: e.target.value })} className={inputClass}>
              {POSITIONS.map((name) => <option key={name}>{name}</option>)}
            </select>
            <RemoveButton onClick={() => removeItem("overlays", i)} />
          </div>
        ))}
      </Section>

      <div className="flex flex-wrap gap-4 text-sm text-gray-300">
        {(
          [
            ["bgm", "BGM"],
            ["sound_effects", "Sound effects"],
            ["subtitles", "Subtitles"],
          ] as const
        ).map(([key, label]) => (
          <label key={key} className="flex items-center gap-2">
            <input type="checkbox" checked={options[key] !== false} onChange={(e) => setOption(key, e.target.checked)} />
            {label}
          </label>
        ))}
        <select value={plan.format ?? "vertical"} onChange={(e) => update({ format: e.target.value })} className={inputClass}>
          <option value="vertical">Vertical 9:16</option>
          <option value="horizontal">Horizontal 16:9</option>
          <option value="square">Square 1:1</option>
        </select>
      </div>

      {error && <p className="text-sm text-red-400">{error}</p>}
      <div className="flex gap-3 justify-end">
        <button
          onClick={() => update({ se_events: [], overlays: [], visual_effects: [] })}
          className="px-4 py-2 rounded-full bg-white/10 hover:bg-white/20 text-sm flex items-center gap-1"
        >
          <Trash2 className="w-4 h-4" /> Clear effects
        </button>
        <button
          disabled={saving || invalid}
          onClick={save}
          className="px-5 py-2 rounded-full bg-gradient-to-r from-purple-500 to-blue-500 font-semibold text-sm flex items-center gap-2 disabled:opacity-50"
        >
          {saving ? <Loader2 className="w-4 h-4 animate-spin" /> : <RefreshCw className="w-4 h-4" />} Re-render
        </button>
      </div>
    </div>
  );
}
