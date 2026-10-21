// node:os

const config = ops.config;
const os = {
  EOL: "\n",
  devNull: "/dev/null",
  constants: {
    signals: internal.signals,
    errno: { ENOENT: 2, EACCES: 13, EEXIST: 17, ENOTDIR: 20, EISDIR: 21, EINVAL: 22 },
    priority: { PRIORITY_LOW: 19, PRIORITY_BELOW_NORMAL: 10, PRIORITY_NORMAL: 0, PRIORITY_ABOVE_NORMAL: -7, PRIORITY_HIGH: -14, PRIORITY_HIGHEST: -20 },
  },
  homedir: () => process.env.HOME || config.homedir,
  tmpdir: () => {
    const t = process.env.TMPDIR || process.env.TMP || process.env.TEMP || config.tmpdir;
    return t.length > 1 && t.endsWith("/") ? t.slice(0, -1) : t;
  },
  hostname: () => config.hostname,
  platform: () => config.platform,
  type: () => config.osType,
  release: () => config.osRelease,
  version: () => config.osVersion,
  machine: () => config.machine,
  arch: () => config.arch,
  endianness: () => "LE",
  uptime: () => ops.osInfo().uptime,
  loadavg: () => ops.osInfo().loadavg,
  totalmem: () => ops.osInfo().totalmem,
  freemem: () => ops.osInfo().freemem,
  availableParallelism: () => ops.osInfo().cpus,
  cpus: () => {
    const info = ops.osInfo();
    return Array.from({ length: info.cpus }, () => ({
      model: info.cpuModel,
      speed: 0,
      times: { user: 0, nice: 0, sys: 0, idle: 0, irq: 0 },
    }));
  },
  networkInterfaces: () => ({}),
  userInfo: (options) => {
    const info = ops.osInfo();
    const u = { uid: info.uid, gid: info.gid, username: info.username, homedir: os.homedir(), shell: info.shell || null };
    if (options?.encoding === "buffer") {
      u.username = internal.Buffer.from(u.username);
      u.homedir = internal.Buffer.from(u.homedir);
    }
    return u;
  },
  getPriority: () => 0,
  setPriority: () => {},
};
internal.defineBuiltin("os", () => os);
