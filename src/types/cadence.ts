export interface Sample {
  id: number;
  file_path: string;
  file_name: string;
  extension: string;
  duration_ms: number | null;
  sample_rate: number | null;
  channels: number | null;
  bpm: number | null;
  pitch_key: string | null;
  created_at: string;
  tag_ids: number[];
}

export interface VirtualTag {
  id: number;
  name: string;
  color_hex: string;
}

export interface ScanResult {
  scanned: number;
  inserted: number;
}
