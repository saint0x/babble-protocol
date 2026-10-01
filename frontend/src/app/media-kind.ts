export type MediaKind = "image" | "audio" | "video";

export const MAX_IMAGE_BYTES = 4 * 1024 * 1024;
export const MAX_PLAYBACK_BYTES = 8 * 1024 * 1024;

const mediaTypes: Readonly<Record<MediaKind, readonly string[]>> = {
  image: ["image/jpeg", "image/png", "image/gif", "image/webp", "image/avif", "image/bmp", "image/x-icon", "image/vnd.microsoft.icon"],
  audio: ["audio/mpeg", "audio/mp4", "audio/aac", "audio/ogg", "audio/wav", "audio/x-wav", "audio/webm", "audio/flac"],
  video: ["video/mp4", "video/webm", "video/ogg", "video/quicktime"],
};

/** Containers we can present safely; decoding still depends on the browser's codecs. */
export function mediaKind(mime: string): MediaKind | null {
  const normalized = mime.toLowerCase();
  for (const kind of ["image", "audio", "video"] as const) {
    if (mediaTypes[kind].includes(normalized)) return kind;
  }
  return null;
}
