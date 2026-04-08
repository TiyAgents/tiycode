export type SystemMetadata = {
  appName: string;
  version: string;
  platform: string;
  arch: string;
  runtime: string;
};

export type InstallSource = "homebrew" | "standalone";
