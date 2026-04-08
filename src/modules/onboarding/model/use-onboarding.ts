import { useCallback, useState } from "react";
import {
  ONBOARDING_COMPLETED_KEY,
  ONBOARDING_STEPS,
  type OnboardingStep,
} from "@/modules/onboarding/model/types";

export function isOnboardingCompleted(): boolean {
  if (typeof window === "undefined") {
    return true;
  }

  return window.localStorage.getItem(ONBOARDING_COMPLETED_KEY) === "true";
}

function markOnboardingCompleted() {
  if (typeof window !== "undefined") {
    window.localStorage.setItem(ONBOARDING_COMPLETED_KEY, "true");
  }
}

export function useOnboarding() {
  const [currentStep, setCurrentStep] = useState<OnboardingStep>("language-theme");
  const [isVisible, setIsVisible] = useState(true);

  const currentIndex = ONBOARDING_STEPS.indexOf(currentStep);

  const goNext = useCallback(() => {
    const nextIndex = currentIndex + 1;

    if (nextIndex >= ONBOARDING_STEPS.length) {
      return;
    }

    setCurrentStep(ONBOARDING_STEPS[nextIndex]);
  }, [currentIndex]);

  const goBack = useCallback(() => {
    const prevIndex = currentIndex - 1;

    if (prevIndex < 0) {
      return;
    }

    setCurrentStep(ONBOARDING_STEPS[prevIndex]);
  }, [currentIndex]);

  const complete = useCallback(() => {
    markOnboardingCompleted();
    setIsVisible(false);
  }, []);

  const skip = useCallback(() => {
    markOnboardingCompleted();
    setIsVisible(false);
  }, []);

  return {
    currentStep,
    currentIndex,
    totalSteps: ONBOARDING_STEPS.length,
    isVisible,
    isFirstStep: currentIndex === 0,
    isLastStep: currentStep === "complete",
    goNext,
    goBack,
    complete,
    skip,
  };
}
