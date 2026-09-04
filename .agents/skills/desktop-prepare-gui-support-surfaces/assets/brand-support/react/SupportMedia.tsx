import { Anchor, Image, Stack } from "@mantine/core";
import type { ReactElement } from "react";

import { isLocalSupportPath } from "./brandSupportProfile";

/** 本地图片的语义或显式装饰声明。 */
export interface LocalImageMedia {
  kind: "image";
  src: string;
  alt: string;
  decorative?: boolean;
}

/** 本地视频必须同时携带字幕轨和可访问文字稿。 */
export interface LocalVideoMedia {
  kind: "video";
  src: string;
  title: string;
  captionsSrc: string;
  captionsLanguage: string;
  captionsLabel: string;
  transcriptHref: string;
  transcriptLabel: string;
  poster?: string;
}

/** 支持界面允许渲染的本地图片或视频。 */
export type SupportMediaDescriptor = LocalImageMedia | LocalVideoMedia;

/** 本地媒体组件的输入。 */
export interface SupportMediaProps {
  media: SupportMediaDescriptor;
}

/** 确保所有媒体引用保持在应用本地打包边界内。 */
function requireLocalPath(value: string, label: string): void {
  if (!isLocalSupportPath(value)) {
    throw new Error(`${label} must use a local absolute path`);
  }
}

/** 渲染具有替代文本的图片，或具有 controls、字幕与文字稿的视频。 */
export function SupportMedia({ media }: SupportMediaProps): ReactElement {
  requireLocalPath(media.src, "support media source");

  if (media.kind === "image") {
    if (media.decorative ? media.alt !== "" : media.alt.trim() === "") {
      throw new Error(
        "support image must have semantic alt text or an explicit empty decorative alt",
      );
    }
    return (
      <Image
        alt={media.alt}
        aria-hidden={media.decorative || undefined}
        fit="contain"
        loading="lazy"
        src={media.src}
      />
    );
  }

  requireLocalPath(media.captionsSrc, "video captions");
  requireLocalPath(media.transcriptHref, "video transcript");
  if (media.poster) {
    requireLocalPath(media.poster, "video poster");
  }

  return (
    <Stack gap="xs">
      <video
        aria-label={media.title}
        controls
        playsInline
        poster={media.poster}
        preload="metadata"
        src={media.src}
        style={{ height: "auto", maxWidth: "100%" }}
      >
        <track
          default
          kind="captions"
          label={media.captionsLabel}
          src={media.captionsSrc}
          srcLang={media.captionsLanguage}
        />
      </video>
      <Anchor href={media.transcriptHref}>{media.transcriptLabel}</Anchor>
    </Stack>
  );
}
