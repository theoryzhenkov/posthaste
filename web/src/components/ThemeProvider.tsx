import {
  applyRootTheme,
  defaultPalettePresetId,
  defaultThemeMode,
  defaultUiDensity,
  designStorageKeys,
  getSystemThemeMode,
  isPalettePresetId,
  isThemeMode,
  isUiDensity,
  type AppliedRootTheme,
  type PalettePresetId,
  type ThemeMode,
  type UiDensity,
} from "@/design";
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  DesignThemeContext,
  type DesignThemeContextValue,
} from "./themeContext";

interface DesignThemeProviderProps {
  children: ReactNode;
}

interface DesignThemePreferences {
  density: UiDensity;
  mode: ThemeMode;
  palettePreset: PalettePresetId;
}

function storedThemeMode(): ThemeMode {
  const value = localStorage.getItem(designStorageKeys.themeMode);
  return value && isThemeMode(value) ? value : defaultThemeMode;
}

function storedPalettePreset(): PalettePresetId {
  const value = localStorage.getItem(designStorageKeys.palettePreset);
  return value && isPalettePresetId(value) ? value : defaultPalettePresetId;
}

function storedDensity(): UiDensity {
  const value = localStorage.getItem(designStorageKeys.uiDensity);
  return value && isUiDensity(value) ? value : defaultUiDensity;
}

function readInitialThemeState(): DesignThemePreferences {
  if (typeof window === "undefined") {
    return {
      mode: defaultThemeMode,
      palettePreset: defaultPalettePresetId,
      density: defaultUiDensity,
    };
  }

  return {
    mode: storedThemeMode(),
    palettePreset: storedPalettePreset(),
    density: storedDensity(),
  };
}

export function DesignThemeProvider({ children }: DesignThemeProviderProps) {
  const [preferences, setPreferences] = useState(readInitialThemeState);
  const { density, mode, palettePreset } = preferences;
  const [applied, setApplied] = useState<AppliedRootTheme>(() => ({
    mode,
    resolvedMode: mode === "dark" ? "dark" : "light",
    palettePreset,
    density,
  }));

  useEffect(() => {
    const root = window.document.documentElement;
    const apply = () =>
      setApplied(applyRootTheme(root, { mode, palettePreset, density }));

    apply();

    if (mode !== "system") {
      return;
    }

    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const handleSystemChange = () =>
      setApplied(
        applyRootTheme(root, { mode, palettePreset, density }, getSystemThemeMode()),
      );

    query.addEventListener("change", handleSystemChange);
    return () => query.removeEventListener("change", handleSystemChange);
  }, [density, mode, palettePreset]);

  const setMode = useCallback((nextMode: ThemeMode) => {
    localStorage.setItem(designStorageKeys.themeMode, nextMode);
    setPreferences((current) => ({ ...current, mode: nextMode }));
  }, []);

  const setPalettePreset = useCallback((nextPreset: PalettePresetId) => {
    localStorage.setItem(designStorageKeys.palettePreset, nextPreset);
    setPreferences((current) => ({ ...current, palettePreset: nextPreset }));
  }, []);

  const setDensity = useCallback((nextDensity: UiDensity) => {
    localStorage.setItem(designStorageKeys.uiDensity, nextDensity);
    setPreferences((current) => ({ ...current, density: nextDensity }));
  }, []);

  const value = useMemo<DesignThemeContextValue>(
    () => ({
      ...applied,
      setDensity,
      setMode,
      setPalettePreset,
    }),
    [applied, setDensity, setMode, setPalettePreset],
  );

  return (
    <DesignThemeContext.Provider value={value}>
      {children}
    </DesignThemeContext.Provider>
  );
}
