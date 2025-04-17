import { useEffect } from "react";
import { useDispatch } from "react-redux";
import { setDarkMode, setLightMode } from "@/store/slices/display";

export function useLightDark() {
  const dispatch = useDispatch();

  useEffect(() => {
    // Only run in the browser
    if (typeof window === "undefined") return;

    const mql = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (e: MediaQueryListEvent) => {
      if (e.matches) dispatch(setDarkMode());
      else dispatch(setLightMode());
    };

    // Initialize
    if (mql.matches) dispatch(setDarkMode());
    else dispatch(setLightMode());

    mql.addEventListener("change", onChange);
    return () => mql.removeEventListener("change", onChange);
  }, [dispatch]);
}
