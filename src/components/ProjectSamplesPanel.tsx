import { useProjectSamples } from "../hooks/useCadence";
import type { Project, ProjectSample } from "../types/cadence";

function folderOf(path: string): string {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i > 0 ? path.slice(0, i) : path;
}

function SampleBadges({ sample }: { sample: ProjectSample }) {
  return (
    <span className="badges">
      {sample.status === "missing" && (
        <span className="badge badge--missing" title="This file isn't at the location the project points to">
          Missing
        </span>
      )}
      {sample.status === "unknown" && (
        <span className="badge badge--muted" title="Couldn't locate FL Studio's data folder to check this file">
          Unchecked
        </span>
      )}
      {sample.status === "factory" && (
        <span className="badge badge--muted" title="Ships with FL Studio">
          Stock
        </span>
      )}
      {sample.duplicate_paths.length > 0 && (
        <span
          className={sample.duplicate_conflict ? "badge badge--conflict" : "badge badge--duplicate"}
          title={`${
            sample.duplicate_conflict
              ? "Same name, but a different size — likely a different recording."
              : "Same name and size — likely a plain copy."
          }\nAlso at:\n${sample.duplicate_paths.join("\n")}`}
        >
          {sample.duplicate_conflict ? "Name conflict" : "Copy"} ×{sample.duplicate_paths.length + 1}
        </span>
      )}
      {sample.status !== "factory" && sample.resolved_path && !sample.in_project_folder && (
        <span className="badge badge--elsewhere" title="Stored outside this project's own folder">
          Outside project folder
        </span>
      )}
    </span>
  );
}

export function ProjectSamplesPanel({ project }: { project: Project | null }) {
  const { data: samples = [], isLoading } = useProjectSamples(project?.id ?? null);

  if (!project) {
    return <div className="project-samples project-samples--empty">Select a project to see its samples</div>;
  }

  const conflicts = samples.filter((s) => s.duplicate_conflict).length;
  const missing = samples.filter((s) => s.status === "missing").length;

  return (
    <div className="project-samples">
      <div className="project-samples__header">
        <div className="project-samples__title" title={project.file_path}>
          {project.file_name}
        </div>
        <div className="project-samples__summary">
          {folderOf(project.file_path)} · {samples.length} samples
          {conflicts > 0 && <span className="summary--warn"> · {conflicts} name conflicts</span>}
          {missing > 0 && <span className="summary--warn"> · {missing} missing</span>}
        </div>
      </div>

      <div className="project-samples__list">
        {isLoading && <div className="project-samples__note">Loading samples...</div>}
        {!isLoading && samples.length === 0 && (
          <div className="project-samples__note">
            No samples found in this project file (it may be empty, or FL Studio saved it in a format that
            couldn't be fully read).
          </div>
        )}
        {samples.map((sample) => (
          <div key={sample.raw_path} className="project-sample">
            <div className="project-sample__main">
              <span className="project-sample__name">{sample.file_name}</span>
              <SampleBadges sample={sample} />
            </div>
            <div className="project-sample__path" title={sample.raw_path}>
              {sample.status === "factory" ? "FL Studio stock library" : (sample.resolved_path ?? sample.raw_path)}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
