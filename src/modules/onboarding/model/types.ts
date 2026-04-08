export type OnboardingStep = "language-theme" | "provider" | "profile" | "complete";

export const ONBOARDING_COMPLETED_KEY = "tiy-agent-onboarding-completed";

export const ONBOARDING_STEPS: ReadonlyArray<OnboardingStep> = [
  "language-theme",
  "provider",
  "profile",
  "complete",
];
