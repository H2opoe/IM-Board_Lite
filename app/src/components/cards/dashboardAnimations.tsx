import { useEffect, useRef, useState } from "react";

export const dashboardIntroAnimationDuration = 1_180;
export const chartIntroAnimationDuration = 760;
export const countUpAnimationDuration = 760;
const dashboardIntroVisibleRatio = 0.3;

type DashboardIntroPhase = "pending" | "active" | "done";

function canPlayDashboardAnimation() {
  return typeof window !== "undefined" && !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function easeOutCubic(progress: number) {
  return 1 - Math.pow(1 - progress, 3);
}

export function useDashboardViewportIntro<T extends HTMLElement>() {
  const ref = useRef<T | null>(null);
  const [phase, setPhase] = useState<DashboardIntroPhase>(() => (canPlayDashboardAnimation() ? "pending" : "done"));

  useEffect(() => {
    if (!canPlayDashboardAnimation()) {
      setPhase("done");
      return undefined;
    }

    const element = ref.current;
    if (!element || typeof IntersectionObserver === "undefined") {
      setPhase("active");
      const timer = window.setTimeout(() => setPhase("done"), dashboardIntroAnimationDuration);
      return () => window.clearTimeout(timer);
    }

    let timer = 0;
    let hasStarted = false;

    function startIntro() {
      if (hasStarted) return;
      hasStarted = true;
      setPhase("active");
      observer.disconnect();
      timer = window.setTimeout(() => setPhase("done"), dashboardIntroAnimationDuration);
    }

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting && entry.intersectionRatio >= dashboardIntroVisibleRatio) {
          startIntro();
        }
      },
      { threshold: [0, dashboardIntroVisibleRatio, 1] }
    );

    observer.observe(element);
    return () => {
      observer.disconnect();
      if (timer) window.clearTimeout(timer);
    };
  }, []);

  return {
    ref,
    hasStarted: phase !== "pending",
    isIntroAnimating: phase === "active",
    className: phase === "pending" ? "is-intro-pending" : phase === "active" ? "is-intro-animating" : ""
  };
}

export function dashboardIntroClassName(baseClassName: string, intro: { className: string }) {
  return [baseClassName, "dashboard-count-trigger", intro.className].filter(Boolean).join(" ");
}

export function useAnimatedNumber(target: number, shouldAnimate: boolean, hasStarted = true, duration = countUpAnimationDuration) {
  const [value, setValue] = useState(() => (hasStarted && !shouldAnimate ? target : 0));
  const hasPlayedIntroRef = useRef(false);

  useEffect(() => {
    if (!hasStarted) {
      setValue(0);
      return undefined;
    }

    if (!shouldAnimate || !canPlayDashboardAnimation()) {
      setValue(target);
      return undefined;
    }

    if (hasPlayedIntroRef.current) {
      setValue(target);
      return undefined;
    }

    hasPlayedIntroRef.current = true;
    setValue(0);
    const startedAt = performance.now();
    let frame = 0;

    function step(now: number) {
      const progress = Math.min((now - startedAt) / duration, 1);
      setValue(target * easeOutCubic(progress));
      if (progress < 1) {
        frame = window.requestAnimationFrame(step);
      }
    }

    frame = window.requestAnimationFrame(step);
    return () => window.cancelAnimationFrame(frame);
  }, [duration, hasStarted, shouldAnimate, target]);

  return Math.round(value);
}

export function CountUpNumber({ value, animate, hasStarted = true }: { value: number; animate: boolean; hasStarted?: boolean }) {
  const animatedValue = useAnimatedNumber(value, animate, hasStarted);
  return <span className="count-up-number">{animatedValue.toLocaleString()}</span>;
}
