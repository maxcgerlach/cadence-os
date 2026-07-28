import { convertFileSrc } from "@tauri-apps/api/core";
import { Play, Pause } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import WaveSurfer from "wavesurfer.js";
import type { Sample } from "../types/cadence";

interface SamplePreviewProps {
  sample: Sample | null;
}

export function SamplePreview({ sample }: SamplePreviewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const waveSurferRef = useRef<WaveSurfer | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isReady, setIsReady] = useState(false);

  // One WaveSurfer instance for the component's lifetime; swap the loaded
  // track when the selected sample changes rather than recreating it.
  useEffect(() => {
    if (!containerRef.current) return;

    const waveSurfer = WaveSurfer.create({
      container: containerRef.current,
      waveColor: "#a8a8b3",
      progressColor: "#396cd8",
      height: 48,
      cursorWidth: 1,
      barWidth: 2,
      barGap: 1,
    });

    waveSurferRef.current = waveSurfer;
    waveSurfer.on("play", () => setIsPlaying(true));
    waveSurfer.on("pause", () => setIsPlaying(false));
    waveSurfer.on("finish", () => setIsPlaying(false));
    waveSurfer.on("ready", () => setIsReady(true));

    return () => {
      waveSurfer.destroy();
      waveSurferRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (!sample || !waveSurferRef.current) return;
    setIsReady(false);
    waveSurferRef.current.load(convertFileSrc(sample.file_path));
  }, [sample?.file_path]);

  if (!sample) {
    return (
      <div className="sample-preview sample-preview--empty">
        Select a sample to preview it
      </div>
    );
  }

  return (
    <div className="sample-preview">
      <button
        className="sample-preview__play"
        disabled={!isReady}
        onClick={() => waveSurferRef.current?.playPause()}
        aria-label={isPlaying ? "Pause" : "Play"}
      >
        {isPlaying ? <Pause size={18} /> : <Play size={18} />}
      </button>
      <div className="sample-preview__body">
        <span className="sample-preview__name" title={sample.file_path}>
          {sample.file_name}
        </span>
        <div ref={containerRef} className="sample-preview__waveform" />
      </div>
    </div>
  );
}
