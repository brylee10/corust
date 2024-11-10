import { useDispatch } from "react-redux";
import { useEffect } from "react";
import { NARROW_SCREEN_PX, setNarrowScreen } from "../store/slices/windowSize";

function WindowSizeListener() {
  const dispatch = useDispatch();

  // Listen for changes in the window size and update the store if it crosses
  // the narrow screen threshold
  useEffect(() => {
    const mediaQuery = window.matchMedia(`(max-width: ${NARROW_SCREEN_PX}px)`);

    // Triggered when the media query match changes
    const handleMediaQueryChange = (event: MediaQueryListEvent) => {
      dispatch(setNarrowScreen(event.matches));
    };

    mediaQuery.addEventListener("change", handleMediaQueryChange);

    // Set initial value
    dispatch(setNarrowScreen(mediaQuery.matches));

    return () =>
      mediaQuery.removeEventListener("change", handleMediaQueryChange);
  }, [dispatch]);

  return null; // This component does not render any UI
}

export default WindowSizeListener;
