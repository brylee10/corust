// Conversions taken from: https://gist.github.com/mjackson/5311256
// Converts an RGB color value to HSL. Conversion formula
// adapted from http://en.wikipedia.org/wiki/HSL_color_space.
function rgbToHsl(rgb: RGBColor): HSLColor {
  const { r, g, b } = rgb;

  // Normalize RGB values to the range [0, 1]
  const red = r / 255;
  const green = g / 255;
  const blue = b / 255;

  const max = Math.max(red, green, blue);
  const min = Math.min(red, green, blue);
  const delta = max - min;

  // Calculate Lightness
  const lightness = (max + min) / 2;

  // Default Hue and Saturation
  let hue = 0;
  let saturation = 0;

  // Calculate Hue and Saturation if not achromatic
  if (delta !== 0) {
    saturation =
      lightness > 0.5 ? delta / (2 - max - min) : delta / (max + min);

    switch (max) {
      case red:
        hue = (green - blue) / delta + (green < blue ? 6 : 0);
        break;
      case green:
        hue = (blue - red) / delta + 2;
        break;
      case blue:
        hue = (red - green) / delta + 4;
        break;
    }

    hue *= 60; // Convert hue to degrees (0-360)
  }

  // Convert saturation and lightness to percentages (0-100)
  const h = Math.round(hue);
  const s = Math.round(saturation * 100);
  const l = Math.round(lightness * 100);

  return { h, s, l };
}

// Converts an HSL color value to RGB. Conversion formula
// adapted from http://en.wikipedia.org/wiki/HSL_color_space.
// returns r, g, and b in the set [0, 255].
function hslToRgb(hsl: HSLColor): RGBColor {
  let r: number, g: number, b: number;
  // Convert HSL values to the range [0, 1]
  const h = hsl.h / 360;
  const s = hsl.s / 100;
  const l = hsl.l / 100;

  if (s === 0) {
    // Achromatic (gray)
    r = g = b = l;
  } else {
    const hue2rgb = (p: number, q: number, t: number): number => {
      if (t < 0) t += 1;
      if (t > 1) t -= 1;
      if (t < 1 / 6) return p + (q - p) * 6 * t;
      if (t < 1 / 2) return q;
      if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
      return p;
    };

    const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
    const p = 2 * l - q;

    r = hue2rgb(p, q, h + 1 / 3);
    g = hue2rgb(p, q, h);
    b = hue2rgb(p, q, h - 1 / 3);
  }

  // Convert RGB values to the range [0, 255]
  return {
    r: Math.round(r * 255),
    g: Math.round(g * 255),
    b: Math.round(b * 255),
  };
}

function parseRgbString(rgbString: string): RGBColor | null {
  // Match the pattern "rgb(r, g, b)"
  const rgbRegex = /^rgb\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*\)$/;

  const match = rgbString.match(rgbRegex);
  if (!match) {
    // Return null if the string does not match the expected format
    console.error("Invalid RGB string: ", rgbString);
    return null;
  }

  const r = parseInt(match[1], 10);
  const g = parseInt(match[2], 10);
  const b = parseInt(match[3], 10);

  // Ensure the values are within the valid range (0-255)
  if (r < 0 || r > 255 || g < 0 || g > 255 || b < 0 || b > 255) {
    // Return null for invalid values
    console.error("Invalid RGB values: ", r, g, b);
    return null;
  }

  return { r, g, b };
}

// Adjusts an HSL color for dark mode.
function darkModeHsl(hsl: HSLColor): HSLColor {
  const { h, s, l } = hsl;
  // Adjust lightness. Make dark colors brighter and light colors darker.
  const isDarkColor = l < 30;
  const adjustedL = isDarkColor ? Math.min(80, l + 40) : Math.max(10, l - 10);
  // Slightly boost saturation to stand out from background, max 90
  const adjustedS = Math.min(80, s + 10);
  // Hue stays the same
  return { h, s: adjustedS, l: adjustedL };
}

function rgbString(rgb: RGBColor): string {
  return `rgb(${rgb.r}, ${rgb.g}, ${rgb.b})`;
}

// Primary utility which takes an RGB color and returns a darkened version of it.
function darkenRgb(rgb: string): string | null {
  const parsedRgb = parseRgbString(rgb);
  if (!parsedRgb) {
    console.error("Invalid RGB string: ", rgb);
    return null;
  }
  const hsl = rgbToHsl(parsedRgb);
  const darkHsl = darkModeHsl(hsl);
  const darkRgb = hslToRgb(darkHsl);
  return rgbString(darkRgb);
}

// Values between [0, 255]
interface RGBColor {
  r: number;
  g: number;
  b: number;
}

// Values between [0, 360] for h, [0, 100] for s and l
interface HSLColor {
  h: number;
  s: number;
  l: number;
}

export { darkenRgb };
