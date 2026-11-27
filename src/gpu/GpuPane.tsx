import { useEffect, useState } from "react";
import { Gpu, GpuReport, closePane, focusPane, gpuStatus } from "../bindings";

type Props = {
  paneId: string;
  focused: boolean;
};

/** How long after one reading the next is asked for. */
const EVERY_MS = 2000;

const SOURCE_LABEL = {
  "nvidia-smi": "nvidia-smi",
  apple: "ioreg",
  none: "No GPU tool",
};

/**
 * This computer's GPUs, read in Rust (cmux-core/src/gpu.rs) from
 * `nvidia-smi` or `ioreg`. Unlike the usage pane it reads again on its
 * own while it is mounted: a GPU's load is only worth seeing live. Each
 * reading is asked for once the last one answered, so a slow tool never
 * has two readings waiting on it.
 */
export function GpuPane({ paneId, focused }: Props) {
  const [report, setReport] = useState<GpuReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    const read = () => {
      void gpuStatus()
        .then((r) => {
          if (!alive) return;
          setReport(r);
          setError(null);
        })
        .catch((e: unknown) => alive && setError(String(e)))
        .finally(() => {
          if (alive) timer = window.setTimeout(read, EVERY_MS);
        });
    };
    read();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, []);

  return (
    <div
      className={`pane usage-pane${focused ? " focused" : ""}`}
      onMouseDown={() => void focusPane(paneId)}
    >
      <div className="graph-chrome">
        <span className="graph-badge">GPU</span>
        <span className="graph-repo">{report ? SOURCE_LABEL[report.source] : "GPUs"}</span>
        <button className="graph-close" title="Close" onClick={() => void closePane(paneId)}>
          ✕
        </button>
      </div>
      {error && <div className="editor-bar error">{error}</div>}
      <div className="usage-body">
        {report === null && !error && <div className="usage-muted">Reading…</div>}
        {report?.gpus.map((gpu) => <GpuCard key={gpu.index} gpu={gpu} />)}
        {report?.error && <div className="usage-muted">{report.error}</div>}
        {report?.source === "none" && (
          <div className="usage-muted">
            This pane looks for nvidia-smi on PATH, and for ioreg on macOS.
          </div>
        )}
      </div>
    </div>
  );
}

function GpuCard({ gpu }: { gpu: Gpu }) {
  const util = gpu.utilizationPercent;
  const memory =
    gpu.memoryUsedBytes === null
      ? null
      : gpu.memoryTotalBytes === null
        ? `${gib(gpu.memoryUsedBytes)} GiB in use`
        : `${gib(gpu.memoryUsedBytes)} / ${gib(gpu.memoryTotalBytes)} GiB`;
  // Most memory first; a process whose memory is not known goes last.
  const processes = [...gpu.processes].sort(
    (a, b) => (b.memoryUsedBytes ?? -1) - (a.memoryUsedBytes ?? -1),
  );
  return (
    <section className="usage-card">
      <div className="usage-head">
        <span className="usage-harness">{gpu.name || `GPU ${gpu.index}`}</span>
        {memory && <span className="usage-login">{memory}</span>}
        {gpu.temperatureC !== null && (
          <span className="usage-login">{Math.round(gpu.temperatureC)}°C</span>
        )}
      </div>
      {util !== null && (
        <div className="usage-limit">
          <span className="usage-limit-name">Busy</span>
          <div className="activity-bar usage-bar">
            <div
              className="activity-bar-fill usage-bar-fill"
              style={{ width: `${Math.min(100, Math.max(0, util))}%` }}
            />
          </div>
          <span className="usage-limit-used">{Math.round(util)}%</span>
        </div>
      )}
      {processes.length > 0 && (
        <table className="gpu-procs">
          <thead>
            <tr>
              <th className="gpu-pid">PID</th>
              <th>Process</th>
              <th className="gpu-mem">Memory</th>
            </tr>
          </thead>
          <tbody>
            {processes.map((p) => (
              <tr key={p.pid}>
                <td className="gpu-pid">{p.pid}</td>
                <td className="gpu-name" title={p.name}>
                  {p.name}
                </td>
                <td className="gpu-mem">
                  {p.memoryUsedBytes === null ? "—" : `${gib(p.memoryUsedBytes)} GiB`}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

function gib(bytes: number): string {
  return (bytes / 2 ** 30).toFixed(1);
}
