import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";
import type { Sample } from "../types/cadence";

const ROW_HEIGHT = 36;

function formatDuration(ms: number | null): string {
  if (ms === null) return "--:--";
  const totalSeconds = Math.round(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

interface SampleTableProps {
  samples: Sample[];
  isLoading: boolean;
  selectedSampleId: number | null;
  onSelect: (sample: Sample) => void;
}

export function SampleTable({ samples, isLoading, selectedSampleId, onSelect }: SampleTableProps) {
  const scrollRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: samples.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  if (isLoading) {
    return <div className="sample-table__empty">Loading samples...</div>;
  }

  if (samples.length === 0) {
    return (
      <div className="sample-table__empty">
        No samples indexed yet. Use "Add Folder to Index" to get started.
      </div>
    );
  }

  const items = virtualizer.getVirtualItems();

  return (
    <div className="sample-table">
      <div className="sample-table__header">
        <span className="col-name">Name</span>
        <span className="col-duration">Duration</span>
        <span className="col-key">Key</span>
        <span className="col-bpm">BPM</span>
      </div>
      <div ref={scrollRef} className="sample-table__scroll">
        <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
          {items.map((virtualRow) => {
            const sample = samples[virtualRow.index];
            const isSelected = sample.id === selectedSampleId;
            return (
              <div
                key={sample.id}
                className={isSelected ? "sample-table__row selected" : "sample-table__row"}
                onClick={() => onSelect(sample)}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  height: virtualRow.size,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
              >
                <span className="col-name" title={sample.file_path}>
                  {sample.file_name}
                </span>
                <span className="col-duration">{formatDuration(sample.duration_ms)}</span>
                <span className="col-key">{sample.pitch_key ?? "--"}</span>
                <span className="col-bpm">{sample.bpm ?? "--"}</span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
