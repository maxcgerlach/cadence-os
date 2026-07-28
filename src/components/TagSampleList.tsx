import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";
import type { Sample } from "../types/cadence";
import { useToggleSampleTag } from "../hooks/useCadence";

const ROW_HEIGHT = 32;

interface TagSampleListProps {
  samples: Sample[];
  tagId: number;
}

export function TagSampleList({ samples, tagId }: TagSampleListProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const toggleTag = useToggleSampleTag();

  const virtualizer = useVirtualizer({
    count: samples.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  if (samples.length === 0) {
    return <div className="sample-table__empty">No samples indexed yet.</div>;
  }

  return (
    <div className="sample-table">
      <div ref={scrollRef} className="sample-table__scroll">
        <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const sample = samples[virtualRow.index];
            const assigned = sample.tag_ids.includes(tagId);
            return (
              <label
                key={sample.id}
                className="tag-assign-row"
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  height: virtualRow.size,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
              >
                <input
                  type="checkbox"
                  checked={assigned}
                  disabled={toggleTag.isPending}
                  onChange={(e) =>
                    toggleTag.mutate({
                      sampleId: sample.id,
                      tagId,
                      assigned: e.target.checked,
                    })
                  }
                />
                <span title={sample.file_path}>{sample.file_name}</span>
              </label>
            );
          })}
        </div>
      </div>
    </div>
  );
}
