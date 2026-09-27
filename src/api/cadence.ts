import { invoke } from "@tauri-apps/api/core";
import type { Project, Sample, ScanResult, VirtualTag } from "../types/cadence";

export function listSamples(search: string): Promise<Sample[]> {
  return invoke("list_samples", { search: search.trim() === "" ? null : search });
}

export function listVirtualTags(): Promise<VirtualTag[]> {
  return invoke("list_virtual_tags");
}

export function scanDirectory(dirPath: string): Promise<ScanResult> {
  return invoke("scan_directory", { dirPath });
}

export function createTag(name: string, colorHex: string): Promise<VirtualTag> {
  return invoke("create_tag", { name, colorHex });
}

export function assignTag(sampleId: number, tagId: number): Promise<void> {
  return invoke("assign_tag", { sampleId, tagId });
}

export function removeTag(sampleId: number, tagId: number): Promise<void> {
  return invoke("remove_tag", { sampleId, tagId });
}

export function listProjects(): Promise<Project[]> {
  return invoke("list_projects");
}
