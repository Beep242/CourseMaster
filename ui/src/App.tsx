import { useEffect, useState } from "react";
import "./App.css";
import invoke from "./api";
import { ensureBridgeSession, portfolioLoginUrl } from "./bridgeAuth";
import type { UserProfile } from "./types";
import { Onboarding } from "./pages/Onboarding";
import { Dashboard } from "./pages/Dashboard";
import { Courses } from "./pages/Courses";
import { CourseDetail } from "./pages/CourseDetail";
import { Settings } from "./pages/Settings";
import { IconBook, IconGear, IconGrid, IconMoon, IconSun } from "./icons";

type Theme = "dark" | "light";
const THEME_KEY = "coursemaster-theme";

function resolveInitialTheme(): Theme {
  const saved = localStorage.getItem(THEME_KEY);
  if (saved === "dark" || saved === "light") return saved;
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

type View = { name: "dashboard" } | { name: "courses" } | { name: "course"; id: string } | { name: "settings" };

const NAV = [
  { key: "dashboard" as const, label: "Dashboard", icon: IconGrid },
  { key: "courses" as const, label: "Courses", icon: IconBook },
  { key: "settings" as const, label: "Settings", icon: IconGear },
];

type AuthState = "checking" | "signed-out" | "signed-in";

function App() {
  const [theme, setTheme] = useState<Theme>(resolveInitialTheme);
  const [authState, setAuthState] = useState<AuthState>("checking");
  const [profile, setProfile] = useState<UserProfile | null | undefined>(undefined);
  const [aiOnline, setAiOnline] = useState<boolean | null>(null);
  const [view, setView] = useState<View>({ name: "dashboard" });

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem(THEME_KEY, theme);
  }, [theme]);

  async function loadProfile() {
    try {
      const p = await invoke<UserProfile | null>("get_profile");
      setProfile(p);
    } catch {
      setProfile(null);
    }
  }

  useEffect(() => {
    ensureBridgeSession()
      .then(() => {
        setAuthState("signed-in");
        loadProfile();
        invoke<boolean>("ai_status")
          .then(setAiOnline)
          .catch(() => setAiOnline(false));
      })
      .catch(() => setAuthState("signed-out"));
  }, []);

  if (authState === "checking") {
    return (
      <div className="onboarding-shell">
        <p className="hint">Signing you in…</p>
      </div>
    );
  }

  if (authState === "signed-out") {
    return (
      <div className="onboarding-shell">
        <div className="card onboarding-card">
          <div className="card-header">
            <div className="card-header-title">
              <IconBook />
              <h3>CourseMaster</h3>
            </div>
          </div>
          <p className="hint">Sign in with your Beep.dev account to continue — CourseMaster uses the same login as the rest of the suite.</p>
          <button type="button" onClick={() => (window.location.href = portfolioLoginUrl())}>
            Sign in with Beep.dev
          </button>
        </div>
      </div>
    );
  }

  if (profile === undefined) {
    return (
      <div className="onboarding-shell">
        <p className="hint">Loading CourseMaster…</p>
      </div>
    );
  }

  if (!profile || !profile.onboarding_complete) {
    return <Onboarding onComplete={loadProfile} />;
  }

  const title = view.name === "course" ? "Course" : (NAV.find((n) => n.key === view.name)?.label ?? "CourseMaster");

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark">
            <IconBook />
          </span>
          <div className="brand-text">
            <span className="brand-name">CourseMaster</span>
            <span className="brand-tag">{profile.name}&rsquo;s semester</span>
          </div>
        </div>

        <nav className="side-nav">
          {NAV.map(({ key, label, icon: Icon }) => (
            <button
              key={key}
              type="button"
              className={`side-item ${view.name === key || (view.name === "course" && key === "courses") ? "active" : ""}`}
              onClick={() => setView({ name: key } as View)}
            >
              <Icon />
              <span>{label}</span>
            </button>
          ))}
        </nav>

        <div className="sidebar-footer">
          <div className={`ai-chip ${aiOnline ? "online" : aiOnline === false ? "offline" : ""}`}>
            <span className="dot" />
            <span>{aiOnline === null ? "Checking AI…" : aiOnline ? "Claude connected" : "AI unavailable"}</span>
          </div>
        </div>
      </aside>

      <div className="shell-main">
        <header className="topbar">
          <h1 className="topbar-title">{title}</h1>
          <div className="topbar-actions">
            <button
              type="button"
              className="theme-toggle"
              onClick={() => setTheme((t) => (t === "dark" ? "light" : "dark"))}
              aria-label="Toggle theme"
              title={theme === "dark" ? "Switch to light mode" : "Switch to dark mode"}
            >
              {theme === "dark" ? <IconSun /> : <IconMoon />}
            </button>
          </div>
        </header>

        <main className="app-main">
          {view.name === "dashboard" && <Dashboard onOpenCourse={(id) => setView({ name: "course", id })} />}
          {view.name === "courses" && <Courses onOpenCourse={(id) => setView({ name: "course", id })} />}
          {view.name === "course" && <CourseDetail courseId={view.id} />}
          {view.name === "settings" && <Settings />}
        </main>
      </div>
    </div>
  );
}

export default App;
