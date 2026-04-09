"use client";

import type { LinkSafetyConfig, LinkSafetyModalProps } from "streamdown";
import { ExternalLinkDialog } from "@/shared/ui/external-link-dialog";

export const streamdownLinkSafety: LinkSafetyConfig = {
  enabled: true,
  renderModal: (props: LinkSafetyModalProps) => <ExternalLinkDialog {...props} />,
};
