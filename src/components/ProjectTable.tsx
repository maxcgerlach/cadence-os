import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";
import type { Project } from "../types/cadence";

const ROW_HEIGHT = 36;

interface ProjectTableProps {
  projects: Project[];
  isLoading: boolean;
  selectedProjectId: number | null;
  onSelect: (project: Project) => void;
}

export function ProjectTable({ projects, isLoading, selectedProjectId, onSelect }: ProjectTableProps) {
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

  return (
    <div className="sample-table">
      <div className="project-table__header">
        <span className="col-name">Project</span>
        <span className="col-bpm">Samples</span>
        <span className="col-bpm">Tempo</span>
      </div>
      <div ref={scrollRef} className="sample-table__scroll">
        <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const project = projects[virtualRow.index];
            const isSelected = project.id === selectedProjectId;
            return (
              <div
                key={project.id}
                className={isSelected ? "project-table__row selected" : "project-table__row"}
                onClick={() => onSelect(project)}
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
                <span className="col-bpm">{project.sample_count}</span>
                <span className="col-bpm">{project.tempo ?? "--"}</span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
