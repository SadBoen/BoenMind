import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ThemeProvider } from "next-themes";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Toaster } from "@/components/ui/sonner";
import "./index.css";
import App from "./App.tsx";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ThemeProvider attribute="class" defaultTheme="system" enableSystem storageKey="boenmind.theme">
      <TooltipProvider delay={300}>
        <App />
        <Toaster position="bottom-right" richColors />
      </TooltipProvider>
    </ThemeProvider>
  </StrictMode>,
);
