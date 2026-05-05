import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, PointerEvent } from "react";
import { EMPTY_STATE_MESSAGES } from "../../constants/messages";
import type { DashboardData } from "../../features/dashboard/model/types";
import { keywordSourcesTitle } from "../../utils/sources";
import { dashboardIntroClassName, useDashboardViewportIntro } from "./dashboardAnimations";

const maxCloudWords = 16;
const cloudAnimationVisibleRatio = 0.2;
const rotationSpeed = 0.00007;
const speedPattern = [0.72, 1.18, 0.88, 1.34, 0.64, 1.52, 1.02, 1.26];
const tiltPattern = [0.18, -0.27, 0.34, -0.13, 0.26, -0.39, 0.08, -0.32];

type CloudMotion = {
  elapsed: number;
  y: number;
  x: number;
};

export function WordCloudCard({ keywords }: { keywords: DashboardData["keywords"] }) {
  const intro = useDashboardViewportIntro<HTMLElement>();
  const cloudRef = useRef<HTMLDivElement>(null);
  const pointerRef = useRef({ x: 0, y: 0 });
  const isCloudVisibleRef = useRef(false);
  const accumulatedElapsedRef = useRef(0);
  const frameRef = useRef(0);
  const runStartedAtRef = useRef(0);
  const [motion, setMotion] = useState<CloudMotion>({ elapsed: 0, y: 0, x: 0 });
  const [isAnimating, setIsAnimating] = useState(false);
  const weights = keywords.map((item) => item.weight);
  const max = Math.max(...weights, 1);
  const min = Math.min(...weights, max);
  const range = Math.max(max - min, 1);
  const visibleKeywords = useMemo(
    () => [...keywords].sort((left, right) => right.weight - left.weight).slice(0, maxCloudWords),
    [keywords]
  );
  const points = useMemo(() => spherePoints(visibleKeywords.length), [visibleKeywords.length]);

  useEffect(() => {
    if (visibleKeywords.length === 0) return undefined;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reducedMotion) {
      setMotion({ elapsed: 0, y: -0.35, x: 0.12 });
      return undefined;
    }

    const cloudElement = cloudRef.current;
    let isRunning = false;
    isCloudVisibleRef.current = false;

    function shouldAnimate() {
      return isCloudVisibleRef.current && document.visibilityState === "visible";
    }

    function stopAnimation(now = performance.now(), shouldUpdateState = true) {
      if (!isRunning) return;
      accumulatedElapsedRef.current += now - runStartedAtRef.current;
      window.cancelAnimationFrame(frameRef.current);
      frameRef.current = 0;
      isRunning = false;
      if (shouldUpdateState) setIsAnimating(false);
    }

    const animate = (now: number) => {
      const elapsed = accumulatedElapsedRef.current + now - runStartedAtRef.current;
      setMotion({
        elapsed,
        y: elapsed * rotationSpeed + pointerRef.current.x * 0.38,
        x: 0.16 + pointerRef.current.y * -0.18
      });
      frameRef.current = window.requestAnimationFrame(animate);
    };

    function startAnimation() {
      if (isRunning || !shouldAnimate()) return;
      runStartedAtRef.current = performance.now();
      isRunning = true;
      setIsAnimating(true);
      frameRef.current = window.requestAnimationFrame(animate);
    }

    function syncAnimationState() {
      if (shouldAnimate()) {
        startAnimation();
      } else {
        stopAnimation();
      }
    }

    // 词云至少露出 20% 才启动动画；底部只露一条时保留静态布局，避免不可见区域持续 RAF 渲染。
    const observer = new IntersectionObserver(
      ([entry]) => {
        isCloudVisibleRef.current = entry.isIntersecting && entry.intersectionRatio >= cloudAnimationVisibleRatio;
        syncAnimationState();
      },
      { threshold: [0, cloudAnimationVisibleRatio] }
    );
    if (cloudElement) observer.observe(cloudElement);
    const handleVisibilityChange = () => syncAnimationState();
    document.addEventListener("visibilitychange", handleVisibilityChange);
    syncAnimationState();

    return () => {
      observer.disconnect();
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      stopAnimation(performance.now(), false);
    };
  }, [visibleKeywords.length]);

  function updatePointer(event: PointerEvent<HTMLDivElement>) {
    const rect = event.currentTarget.getBoundingClientRect();
    const x = ((event.clientX - rect.left) / rect.width - 0.5) * 2;
    const y = ((event.clientY - rect.top) / rect.height - 0.5) * 2;
    pointerRef.current = { x, y };
    event.currentTarget.style.setProperty("--cloud-x", x.toFixed(3));
    event.currentTarget.style.setProperty("--cloud-y", y.toFixed(3));
  }

  function resetPointer() {
    pointerRef.current = { x: 0, y: 0 };
    cloudRef.current?.style.setProperty("--cloud-x", "0");
    cloudRef.current?.style.setProperty("--cloud-y", "0");
  }

  return (
    <article ref={intro.ref} className={dashboardIntroClassName("panel word-cloud-panel", intro)}>
      <header className="panel-header">
        <div>
          <strong>关键词词云</strong>
          <span>今日热词</span>
        </div>
      </header>
      <div
        className="word-cloud"
        ref={cloudRef}
        data-animating={isAnimating ? "true" : undefined}
        onPointerMove={updatePointer}
        onPointerLeave={resetPointer}
      >
        {keywords.length === 0 ? (
          <div className="empty-state">{EMPTY_STATE_MESSAGES.noKeywords}</div>
        ) : (
          visibleKeywords.map((keyword, index) => {
            const normalized = (keyword.weight - min) / range;
            const keywordCount =
              keyword.count ?? keyword.sources?.reduce((sum, source) => sum + source.count, 0) ?? keyword.weight;
            const point = keywordMotionPoint(points[index], motion, index);
            const depth = (point.z + 1) / 2;
            const weightedScale = 0.9 + Math.pow(normalized, 0.72) * 0.5;
            const depthScale = 0.92 + depth * 0.2;
            const keywordScale = Math.min(weightedScale * depthScale, 1.48);
            return (
              <button
                key={keyword.text}
                className="keyword"
                title={`${keyword.text}：${keywordCount}次\n${keywordSourcesTitle(keyword.sources)}`}
                style={{
                  left: `${50 + point.x * 36}%`,
                  top: `${50 + point.y * 31}%`,
                  "--keyword-scale": keywordScale,
                  "--keyword-depth": 0.42 + depth * 0.9,
                  "--keyword-opacity": 0.45 + depth * 0.55,
                  zIndex: Math.round(depth * 100)
                } as CSSProperties}
              >
                {keyword.text}
              </button>
            );
          })
        )}
      </div>
    </article>
  );
}

function spherePoints(count: number) {
  if (count <= 0) return [];
  const goldenAngle = Math.PI * (3 - Math.sqrt(5));
  if (count === 1) return [{ x: 0, y: -0.08, z: 1 }];
  return Array.from({ length: count }, (_, index) => {
    if (index === 0) return { x: -0.18, y: -0.08, z: 0.98 };
    const offsetIndex = index - 1;
    const offsetCount = count - 1;
    const y = 1 - ((offsetIndex + 0.5) / offsetCount) * 2;
    const radius = Math.sqrt(Math.max(0, 1 - y * y));
    const theta = offsetIndex * goldenAngle;
    return {
      x: Math.cos(theta) * radius,
      y,
      z: Math.sin(theta) * radius
    };
  });
}

function rotatePoint(point: { x: number; y: number; z: number }, rotation: { x: number; y: number }) {
  const cosY = Math.cos(rotation.y);
  const sinY = Math.sin(rotation.y);
  const x1 = point.x * cosY + point.z * sinY;
  const z1 = point.z * cosY - point.x * sinY;
  const cosX = Math.cos(rotation.x);
  const sinX = Math.sin(rotation.x);
  return {
    x: x1,
    y: point.y * cosX - z1 * sinX,
    z: point.y * sinX + z1 * cosX
  };
}

function keywordMotionPoint(point: { x: number; y: number; z: number }, motion: CloudMotion, index: number) {
  const speed = speedPattern[index % speedPattern.length];
  const nextSpeed = speedPattern[(index + 1) % speedPattern.length];
  const phase = index * 1.731 + (index % 2 === 0 ? 0.37 : 1.04);
  const driftDirection = index % 2 === 0 ? 1 : -1;
  const driftSpeed = 0.00009 + (index % 5) * 0.000018;
  const driftAngle = motion.elapsed * driftSpeed * driftDirection + phase;
  const localPoint = rotatePoint(point, {
    y: motion.y * speed + phase * 0.12,
    x:
      motion.x +
      tiltPattern[index % tiltPattern.length] +
      Math.sin(motion.elapsed * (0.00008 + nextSpeed * 0.000014) + phase) * 0.08
  });

  return {
    x: clampCloudAxis(localPoint.x + Math.cos(driftAngle) * (0.035 + (index % 3) * 0.01)),
    y: clampCloudAxis(localPoint.y + Math.sin(driftAngle * (1.18 + speed * 0.08)) * (0.028 + (index % 4) * 0.008)),
    z: clampCloudAxis(localPoint.z + Math.sin(driftAngle * 0.72) * 0.06)
  };
}

function clampCloudAxis(value: number) {
  return Math.max(-1, Math.min(1, value));
}
