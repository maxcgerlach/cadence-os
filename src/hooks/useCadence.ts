import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  assignTag,
  createTag,
  listProjects,
  listSamples,
  listVirtualTags,
  removeTag,
  scanDirectory,
} from "../api/cadence";

export function useSamples(search: string) {
  return useQuery({
    queryKey: ["samples", search],
    queryFn: () => listSamples(search),
  });
}

export function useVirtualTags() {
  return useQuery({
    queryKey: ["virtual-tags"],
    queryFn: listVirtualTags,
  });
}

export function useProjects() {
  return useQuery({
    queryKey: ["projects"],
    queryFn: listProjects,
  });
}

export function useScanDirectory() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: scanDirectory,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["samples"] });
      queryClient.invalidateQueries({ queryKey: ["projects"] });
    },
  });
}

export function useCreateTag() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ name, colorHex }: { name: string; colorHex: string }) =>
      createTag(name, colorHex),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["virtual-tags"] });
    },
  });
}

export function useToggleSampleTag() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      sampleId,
      tagId,
      assigned,
    }: {
      sampleId: number;
      tagId: number;
      assigned: boolean;
    }) => (assigned ? assignTag(sampleId, tagId) : removeTag(sampleId, tagId)),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["samples"] });
    },
  });
}
