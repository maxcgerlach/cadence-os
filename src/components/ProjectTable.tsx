import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";
import type { Project } from "../types/cadence";

const ROW_HEIGHT = 36;

interface ProjectTableProps {
  projects: Project[];
  isLoading: boolean;
}

export function ProjectTable({ projects, isLoading }: ProjectTableProps) {
  const scrollRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer({
    count: projects.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  if (isLoading) {
    return <div className="sample-table__empty">Loading projects...</div>;
  }

  if (projects.length === 0) {
    return (
      <div className="sample-table__empty">
        No FL Studio projects indexed yet. Use "Add Folder to Index" to get started.
      </div>
    );
  }

  const items = virtualizer.getVirtualItems();

  return (
    <div className="sample-table">
      <div className="project-table__header">
        <span className="col-name">Name</span>
        <span className="col-bpm">Tempo</span>
      </div>
      <div ref={scrollRef} className="sample-table__scroll">
        <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
          {items.map((virtualRow) => {
            const project = projects[virtualRow.index];
            return (
              <div
                key={project.id}
                className="project-table__row"
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  height: virtualRow.size,
                  transform: `translateY(${virtualRow.start}px)`,
                }}
              >
                <span className="col-name" title={project.file_path}>
                  {project.file_name}
                </span>
                <span className="col-bpm">{project.tempo ?? "--"}</span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
