import { clearSession, ensureBridgeSession, exchangeBridgeSession, storeSession } from "./bridgeAuth";

const API_BASE = import.meta.env.VITE_API_BASE_URL ?? "https://coursemaster.iambeep.com/api";

type Req = { method: string; path: string; body?: unknown };

/// Every Tauri-invoke-style call site in the app (`invoke("command_name", {...})`)
/// stays unchanged here — this table is the only place that knows CourseMaster
/// grew a real HTTP API instead of Tauri IPC, so the page components never had
/// to be rewritten for the switch.
function resolveRequest(cmd: string, args: Record<string, unknown>): Req {
  switch (cmd) {
    case "get_profile":
      return { method: "GET", path: "/profile" };
    case "save_profile":
      return { method: "PUT", path: "/profile", body: args.profileInput };
    case "create_semester":
      return { method: "POST", path: "/semesters", body: args.input };
    case "list_semesters":
      return { method: "GET", path: "/semesters" };
    case "create_course":
      return { method: "POST", path: "/courses", body: args.input };
    case "list_courses":
      return { method: "GET", path: withQuery("/courses", { semester_id: args.semesterId }) };
    case "get_course":
      return { method: "GET", path: `/courses/${encodeURIComponent(String(args.id))}` };
    case "update_course_grade":
      return { method: "PATCH", path: `/courses/${encodeURIComponent(String(args.id))}/grade`, body: { grade: args.grade } };
    case "create_assignment":
      return { method: "POST", path: "/assignments", body: args.input };
    case "list_assignments":
      return { method: "GET", path: withQuery("/assignments", { course_id: args.courseId }) };
    case "update_assignment":
      return { method: "PATCH", path: `/assignments/${encodeURIComponent(String(args.id))}`, body: args.patch };
    case "delete_assignment":
      return { method: "DELETE", path: `/assignments/${encodeURIComponent(String(args.id))}` };
    case "create_subtask":
      return { method: "POST", path: "/subtasks", body: args.input };
    case "list_subtasks":
      return { method: "GET", path: `/assignments/${encodeURIComponent(String(args.assignmentId))}/subtasks` };
    case "submit_syllabus":
      return { method: "POST", path: "/syllabi", body: { course_id: args.courseId, raw_text: args.rawText } };
    case "list_syllabi":
      return { method: "GET", path: `/courses/${encodeURIComponent(String(args.courseId))}/syllabi` };
    case "list_extractions":
      return { method: "GET", path: `/syllabi/${encodeURIComponent(String(args.syllabusId))}/extractions` };
    case "approve_extraction":
      return {
        method: "POST",
        path: `/extractions/${encodeURIComponent(String(args.extractionId))}/approve`,
        body: { edits: args.edits ?? null },
      };
    case "reject_extraction":
      return { method: "POST", path: `/extractions/${encodeURIComponent(String(args.extractionId))}/reject` };
    case "ask_syllabus":
      return { method: "POST", path: `/syllabi/${encodeURIComponent(String(args.syllabusId))}/ask`, body: { question: args.question } };
    case "prioritized_today":
      return { method: "GET", path: "/prioritized" };
    case "ai_status":
      return { method: "GET", path: "/ai/status" };
    default:
      throw new Error(`Unknown command: ${cmd}`);
  }
}

function withQuery(path: string, params: Record<string, unknown>): string {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null) search.set(key, String(value));
  }
  const qs = search.toString();
  return qs ? `${path}?${qs}` : path;
}

class ApiHttpError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function doFetch<T>(req: Req, token: string): Promise<T> {
  const res = await fetch(`${API_BASE}${req.path}`, {
    method: req.method,
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: req.body !== undefined && req.method !== "GET" ? JSON.stringify(req.body) : undefined,
  });
  const text = await res.text();
  const data = text ? JSON.parse(text) : undefined;
  if (!res.ok) {
    throw new ApiHttpError(res.status, (data && data.error) || `${req.method} ${req.path} failed (${res.status})`);
  }
  return data as T;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const req = resolveRequest(cmd, args ?? {});
  const session = await ensureBridgeSession();
  try {
    return await doFetch<T>(req, session.accessToken);
  } catch (err) {
    // A 401 here means the cached bridge token expired mid-session — clear it
    // and try exactly once more with a freshly minted one before giving up.
    if (err instanceof ApiHttpError && err.status === 401) {
      clearSession();
      const fresh = await exchangeBridgeSession();
      storeSession(fresh);
      return doFetch<T>(req, fresh.accessToken);
    }
    throw err;
  }
}

export default invoke;
