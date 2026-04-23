import { useContext } from "react";
import { DesignThemeContext } from "../components/themeContext";

export function useDesignTheme() {
  const context = useContext(DesignThemeContext);
  if (!context) {
    throw new Error("useDesignTheme must be used within DesignThemeProvider");
  }
  return context;
}
