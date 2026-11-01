import { useEffect, useRef } from "react";

/** 変更イベントごとに呼ばれる。短時間の連続イベントは1回の再読み込みにまとめる。 */
export function useLiveChanges(onChange: () => void): void {
  const handler = useRef(onChange);
  handler.current = onChange;
  useEffect(() => {
    let timer: number | undefined;
    const source = new EventSource("/api/v1/events/stream?after=latest");
    source.onmessage = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => handler.current(), 150);
    };
    return () => {
      window.clearTimeout(timer);
      source.close();
    };
  }, []);
}
