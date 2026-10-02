var __defProp = Object.defineProperty;
var __name = (target, value) => __defProp(target, "name", { value, configurable: true });

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/unenv/dist/runtime/_internal/utils.mjs
// @__NO_SIDE_EFFECTS__
function createNotImplementedError(name) {
  return new Error(`[unenv] ${name} is not implemented yet!`);
}
__name(createNotImplementedError, "createNotImplementedError");
// @__NO_SIDE_EFFECTS__
function notImplemented(name) {
  const fn = /* @__PURE__ */ __name(() => {
    throw /* @__PURE__ */ createNotImplementedError(name);
  }, "fn");
  return Object.assign(fn, { __unenv__: true });
}
__name(notImplemented, "notImplemented");
// @__NO_SIDE_EFFECTS__
function notImplementedClass(name) {
  return class {
    __unenv__ = true;
    constructor() {
      throw new Error(`[unenv] ${name} is not implemented yet!`);
    }
  };
}
__name(notImplementedClass, "notImplementedClass");

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/unenv/dist/runtime/node/internal/perf_hooks/performance.mjs
var _timeOrigin = globalThis.performance?.timeOrigin ?? Date.now();
var _performanceNow = globalThis.performance?.now ? globalThis.performance.now.bind(globalThis.performance) : () => Date.now() - _timeOrigin;
var nodeTiming = {
  name: "node",
  entryType: "node",
  startTime: 0,
  duration: 0,
  nodeStart: 0,
  v8Start: 0,
  bootstrapComplete: 0,
  environment: 0,
  loopStart: 0,
  loopExit: 0,
  idleTime: 0,
  uvMetricsInfo: {
    loopCount: 0,
    events: 0,
    eventsWaiting: 0
  },
  detail: void 0,
  toJSON() {
    return this;
  }
};
var PerformanceEntry = class {
  static {
    __name(this, "PerformanceEntry");
  }
  __unenv__ = true;
  detail;
  entryType = "event";
  name;
  startTime;
  constructor(name, options) {
    this.name = name;
    this.startTime = options?.startTime || _performanceNow();
    this.detail = options?.detail;
  }
  get duration() {
    return _performanceNow() - this.startTime;
  }
  toJSON() {
    return {
      name: this.name,
      entryType: this.entryType,
      startTime: this.startTime,
      duration: this.duration,
      detail: this.detail
    };
  }
};
var PerformanceMark = class PerformanceMark2 extends PerformanceEntry {
  static {
    __name(this, "PerformanceMark");
  }
  entryType = "mark";
  constructor() {
    super(...arguments);
  }
  get duration() {
    return 0;
  }
};
var PerformanceMeasure = class extends PerformanceEntry {
  static {
    __name(this, "PerformanceMeasure");
  }
  entryType = "measure";
};
var PerformanceResourceTiming = class extends PerformanceEntry {
  static {
    __name(this, "PerformanceResourceTiming");
  }
  entryType = "resource";
  serverTiming = [];
  connectEnd = 0;
  connectStart = 0;
  decodedBodySize = 0;
  domainLookupEnd = 0;
  domainLookupStart = 0;
  encodedBodySize = 0;
  fetchStart = 0;
  initiatorType = "";
  name = "";
  nextHopProtocol = "";
  redirectEnd = 0;
  redirectStart = 0;
  requestStart = 0;
  responseEnd = 0;
  responseStart = 0;
  secureConnectionStart = 0;
  startTime = 0;
  transferSize = 0;
  workerStart = 0;
  responseStatus = 0;
};
var PerformanceObserverEntryList = class {
  static {
    __name(this, "PerformanceObserverEntryList");
  }
  __unenv__ = true;
  getEntries() {
    return [];
  }
  getEntriesByName(_name, _type) {
    return [];
  }
  getEntriesByType(type) {
    return [];
  }
};
var Performance = class {
  static {
    __name(this, "Performance");
  }
  __unenv__ = true;
  timeOrigin = _timeOrigin;
  eventCounts = /* @__PURE__ */ new Map();
  _entries = [];
  _resourceTimingBufferSize = 0;
  navigation = void 0;
  timing = void 0;
  timerify(_fn, _options) {
    throw createNotImplementedError("Performance.timerify");
  }
  get nodeTiming() {
    return nodeTiming;
  }
  eventLoopUtilization() {
    return {};
  }
  markResourceTiming() {
    return new PerformanceResourceTiming("");
  }
  onresourcetimingbufferfull = null;
  now() {
    if (this.timeOrigin === _timeOrigin) {
      return _performanceNow();
    }
    return Date.now() - this.timeOrigin;
  }
  clearMarks(markName) {
    this._entries = markName ? this._entries.filter((e) => e.name !== markName) : this._entries.filter((e) => e.entryType !== "mark");
  }
  clearMeasures(measureName) {
    this._entries = measureName ? this._entries.filter((e) => e.name !== measureName) : this._entries.filter((e) => e.entryType !== "measure");
  }
  clearResourceTimings() {
    this._entries = this._entries.filter((e) => e.entryType !== "resource" || e.entryType !== "navigation");
  }
  getEntries() {
    return this._entries;
  }
  getEntriesByName(name, type) {
    return this._entries.filter((e) => e.name === name && (!type || e.entryType === type));
  }
  getEntriesByType(type) {
    return this._entries.filter((e) => e.entryType === type);
  }
  mark(name, options) {
    const entry = new PerformanceMark(name, options);
    this._entries.push(entry);
    return entry;
  }
  measure(measureName, startOrMeasureOptions, endMark) {
    let start;
    let end;
    if (typeof startOrMeasureOptions === "string") {
      start = this.getEntriesByName(startOrMeasureOptions, "mark")[0]?.startTime;
      end = this.getEntriesByName(endMark, "mark")[0]?.startTime;
    } else {
      start = Number.parseFloat(startOrMeasureOptions?.start) || this.now();
      end = Number.parseFloat(startOrMeasureOptions?.end) || this.now();
    }
    const entry = new PerformanceMeasure(measureName, {
      startTime: start,
      detail: {
        start,
        end
      }
    });
    this._entries.push(entry);
    return entry;
  }
  setResourceTimingBufferSize(maxSize) {
    this._resourceTimingBufferSize = maxSize;
  }
  addEventListener(type, listener, options) {
    throw createNotImplementedError("Performance.addEventListener");
  }
  removeEventListener(type, listener, options) {
    throw createNotImplementedError("Performance.removeEventListener");
  }
  dispatchEvent(event) {
    throw createNotImplementedError("Performance.dispatchEvent");
  }
  toJSON() {
    return this;
  }
};
var PerformanceObserver = class {
  static {
    __name(this, "PerformanceObserver");
  }
  __unenv__ = true;
  static supportedEntryTypes = [];
  _callback = null;
  constructor(callback) {
    this._callback = callback;
  }
  takeRecords() {
    return [];
  }
  disconnect() {
    throw createNotImplementedError("PerformanceObserver.disconnect");
  }
  observe(options) {
    throw createNotImplementedError("PerformanceObserver.observe");
  }
  bind(fn) {
    return fn;
  }
  runInAsyncScope(fn, thisArg, ...args) {
    return fn.call(thisArg, ...args);
  }
  asyncId() {
    return 0;
  }
  triggerAsyncId() {
    return 0;
  }
  emitDestroy() {
    return this;
  }
};
var performance = globalThis.performance && "addEventListener" in globalThis.performance ? globalThis.performance : new Performance();

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/@cloudflare/unenv-preset/dist/runtime/polyfill/performance.mjs
if (!("__unenv__" in performance)) {
  const proto = Performance.prototype;
  for (const key of Object.getOwnPropertyNames(proto)) {
    if (key !== "constructor" && !(key in performance)) {
      const desc = Object.getOwnPropertyDescriptor(proto, key);
      if (desc) {
        Object.defineProperty(performance, key, desc);
      }
    }
  }
}
globalThis.performance = performance;
globalThis.Performance = Performance;
globalThis.PerformanceEntry = PerformanceEntry;
globalThis.PerformanceMark = PerformanceMark;
globalThis.PerformanceMeasure = PerformanceMeasure;
globalThis.PerformanceObserver = PerformanceObserver;
globalThis.PerformanceObserverEntryList = PerformanceObserverEntryList;
globalThis.PerformanceResourceTiming = PerformanceResourceTiming;

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/unenv/dist/runtime/node/console.mjs
import { Writable } from "node:stream";

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/unenv/dist/runtime/mock/noop.mjs
var noop_default = Object.assign(() => {
}, { __unenv__: true });

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/unenv/dist/runtime/node/console.mjs
var _console = globalThis.console;
var _ignoreErrors = true;
var _stderr = new Writable();
var _stdout = new Writable();
var log = _console?.log ?? noop_default;
var info = _console?.info ?? log;
var trace = _console?.trace ?? info;
var debug = _console?.debug ?? log;
var table = _console?.table ?? log;
var error = _console?.error ?? log;
var warn = _console?.warn ?? error;
var createTask = _console?.createTask ?? /* @__PURE__ */ notImplemented("console.createTask");
var clear = _console?.clear ?? noop_default;
var count = _console?.count ?? noop_default;
var countReset = _console?.countReset ?? noop_default;
var dir = _console?.dir ?? noop_default;
var dirxml = _console?.dirxml ?? noop_default;
var group = _console?.group ?? noop_default;
var groupEnd = _console?.groupEnd ?? noop_default;
var groupCollapsed = _console?.groupCollapsed ?? noop_default;
var profile = _console?.profile ?? noop_default;
var profileEnd = _console?.profileEnd ?? noop_default;
var time = _console?.time ?? noop_default;
var timeEnd = _console?.timeEnd ?? noop_default;
var timeLog = _console?.timeLog ?? noop_default;
var timeStamp = _console?.timeStamp ?? noop_default;
var Console = _console?.Console ?? /* @__PURE__ */ notImplementedClass("console.Console");
var _times = /* @__PURE__ */ new Map();
var _stdoutErrorHandler = noop_default;
var _stderrErrorHandler = noop_default;

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/@cloudflare/unenv-preset/dist/runtime/node/console.mjs
var workerdConsole = globalThis["console"];
var {
  assert,
  clear: clear2,
  // @ts-expect-error undocumented public API
  context,
  count: count2,
  countReset: countReset2,
  // @ts-expect-error undocumented public API
  createTask: createTask2,
  debug: debug2,
  dir: dir2,
  dirxml: dirxml2,
  error: error2,
  group: group2,
  groupCollapsed: groupCollapsed2,
  groupEnd: groupEnd2,
  info: info2,
  log: log2,
  profile: profile2,
  profileEnd: profileEnd2,
  table: table2,
  time: time2,
  timeEnd: timeEnd2,
  timeLog: timeLog2,
  timeStamp: timeStamp2,
  trace: trace2,
  warn: warn2
} = workerdConsole;
Object.assign(workerdConsole, {
  Console,
  _ignoreErrors,
  _stderr,
  _stderrErrorHandler,
  _stdout,
  _stdoutErrorHandler,
  _times
});
var console_default = workerdConsole;

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/wrangler/_virtual_unenv_global_polyfill-@cloudflare-unenv-preset-node-console
globalThis.console = console_default;

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/unenv/dist/runtime/node/internal/process/hrtime.mjs
var hrtime = /* @__PURE__ */ Object.assign(/* @__PURE__ */ __name(function hrtime2(startTime) {
  const now = Date.now();
  const seconds = Math.trunc(now / 1e3);
  const nanos = now % 1e3 * 1e6;
  if (startTime) {
    let diffSeconds = seconds - startTime[0];
    let diffNanos = nanos - startTime[0];
    if (diffNanos < 0) {
      diffSeconds = diffSeconds - 1;
      diffNanos = 1e9 + diffNanos;
    }
    return [diffSeconds, diffNanos];
  }
  return [seconds, nanos];
}, "hrtime"), { bigint: /* @__PURE__ */ __name(function bigint() {
  return BigInt(Date.now() * 1e6);
}, "bigint") });

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/unenv/dist/runtime/node/internal/process/process.mjs
import { EventEmitter } from "node:events";

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/unenv/dist/runtime/node/internal/tty/read-stream.mjs
var ReadStream = class {
  static {
    __name(this, "ReadStream");
  }
  fd;
  isRaw = false;
  isTTY = false;
  constructor(fd) {
    this.fd = fd;
  }
  setRawMode(mode) {
    this.isRaw = mode;
    return this;
  }
};

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/unenv/dist/runtime/node/internal/tty/write-stream.mjs
var WriteStream = class {
  static {
    __name(this, "WriteStream");
  }
  fd;
  columns = 80;
  rows = 24;
  isTTY = false;
  constructor(fd) {
    this.fd = fd;
  }
  clearLine(dir3, callback) {
    callback && callback();
    return false;
  }
  clearScreenDown(callback) {
    callback && callback();
    return false;
  }
  cursorTo(x, y, callback) {
    callback && typeof callback === "function" && callback();
    return false;
  }
  moveCursor(dx, dy, callback) {
    callback && callback();
    return false;
  }
  getColorDepth(env2) {
    return 1;
  }
  hasColors(count3, env2) {
    return false;
  }
  getWindowSize() {
    return [this.columns, this.rows];
  }
  write(str, encoding, cb) {
    if (str instanceof Uint8Array) {
      str = new TextDecoder().decode(str);
    }
    try {
      console.log(str);
    } catch {
    }
    cb && typeof cb === "function" && cb();
    return false;
  }
};

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/unenv/dist/runtime/node/internal/process/node-version.mjs
var NODE_VERSION = "22.14.0";

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/unenv/dist/runtime/node/internal/process/process.mjs
var Process = class _Process extends EventEmitter {
  static {
    __name(this, "Process");
  }
  env;
  hrtime;
  nextTick;
  constructor(impl) {
    super();
    this.env = impl.env;
    this.hrtime = impl.hrtime;
    this.nextTick = impl.nextTick;
    for (const prop of [...Object.getOwnPropertyNames(_Process.prototype), ...Object.getOwnPropertyNames(EventEmitter.prototype)]) {
      const value = this[prop];
      if (typeof value === "function") {
        this[prop] = value.bind(this);
      }
    }
  }
  // --- event emitter ---
  emitWarning(warning, type, code) {
    console.warn(`${code ? `[${code}] ` : ""}${type ? `${type}: ` : ""}${warning}`);
  }
  emit(...args) {
    return super.emit(...args);
  }
  listeners(eventName) {
    return super.listeners(eventName);
  }
  // --- stdio (lazy initializers) ---
  #stdin;
  #stdout;
  #stderr;
  get stdin() {
    return this.#stdin ??= new ReadStream(0);
  }
  get stdout() {
    return this.#stdout ??= new WriteStream(1);
  }
  get stderr() {
    return this.#stderr ??= new WriteStream(2);
  }
  // --- cwd ---
  #cwd = "/";
  chdir(cwd2) {
    this.#cwd = cwd2;
  }
  cwd() {
    return this.#cwd;
  }
  // --- dummy props and getters ---
  arch = "";
  platform = "";
  argv = [];
  argv0 = "";
  execArgv = [];
  execPath = "";
  title = "";
  pid = 200;
  ppid = 100;
  get version() {
    return `v${NODE_VERSION}`;
  }
  get versions() {
    return { node: NODE_VERSION };
  }
  get allowedNodeEnvironmentFlags() {
    return /* @__PURE__ */ new Set();
  }
  get sourceMapsEnabled() {
    return false;
  }
  get debugPort() {
    return 0;
  }
  get throwDeprecation() {
    return false;
  }
  get traceDeprecation() {
    return false;
  }
  get features() {
    return {};
  }
  get release() {
    return {};
  }
  get connected() {
    return false;
  }
  get config() {
    return {};
  }
  get moduleLoadList() {
    return [];
  }
  constrainedMemory() {
    return 0;
  }
  availableMemory() {
    return 0;
  }
  uptime() {
    return 0;
  }
  resourceUsage() {
    return {};
  }
  // --- noop methods ---
  ref() {
  }
  unref() {
  }
  // --- unimplemented methods ---
  umask() {
    throw createNotImplementedError("process.umask");
  }
  getBuiltinModule() {
    return void 0;
  }
  getActiveResourcesInfo() {
    throw createNotImplementedError("process.getActiveResourcesInfo");
  }
  exit() {
    throw createNotImplementedError("process.exit");
  }
  reallyExit() {
    throw createNotImplementedError("process.reallyExit");
  }
  kill() {
    throw createNotImplementedError("process.kill");
  }
  abort() {
    throw createNotImplementedError("process.abort");
  }
  dlopen() {
    throw createNotImplementedError("process.dlopen");
  }
  setSourceMapsEnabled() {
    throw createNotImplementedError("process.setSourceMapsEnabled");
  }
  loadEnvFile() {
    throw createNotImplementedError("process.loadEnvFile");
  }
  disconnect() {
    throw createNotImplementedError("process.disconnect");
  }
  cpuUsage() {
    throw createNotImplementedError("process.cpuUsage");
  }
  setUncaughtExceptionCaptureCallback() {
    throw createNotImplementedError("process.setUncaughtExceptionCaptureCallback");
  }
  hasUncaughtExceptionCaptureCallback() {
    throw createNotImplementedError("process.hasUncaughtExceptionCaptureCallback");
  }
  initgroups() {
    throw createNotImplementedError("process.initgroups");
  }
  openStdin() {
    throw createNotImplementedError("process.openStdin");
  }
  assert() {
    throw createNotImplementedError("process.assert");
  }
  binding() {
    throw createNotImplementedError("process.binding");
  }
  // --- attached interfaces ---
  permission = { has: /* @__PURE__ */ notImplemented("process.permission.has") };
  report = {
    directory: "",
    filename: "",
    signal: "SIGUSR2",
    compact: false,
    reportOnFatalError: false,
    reportOnSignal: false,
    reportOnUncaughtException: false,
    getReport: /* @__PURE__ */ notImplemented("process.report.getReport"),
    writeReport: /* @__PURE__ */ notImplemented("process.report.writeReport")
  };
  finalization = {
    register: /* @__PURE__ */ notImplemented("process.finalization.register"),
    unregister: /* @__PURE__ */ notImplemented("process.finalization.unregister"),
    registerBeforeExit: /* @__PURE__ */ notImplemented("process.finalization.registerBeforeExit")
  };
  memoryUsage = Object.assign(() => ({
    arrayBuffers: 0,
    rss: 0,
    external: 0,
    heapTotal: 0,
    heapUsed: 0
  }), { rss: /* @__PURE__ */ __name(() => 0, "rss") });
  // --- undefined props ---
  mainModule = void 0;
  domain = void 0;
  // optional
  send = void 0;
  exitCode = void 0;
  channel = void 0;
  getegid = void 0;
  geteuid = void 0;
  getgid = void 0;
  getgroups = void 0;
  getuid = void 0;
  setegid = void 0;
  seteuid = void 0;
  setgid = void 0;
  setgroups = void 0;
  setuid = void 0;
  // internals
  _events = void 0;
  _eventsCount = void 0;
  _exiting = void 0;
  _maxListeners = void 0;
  _debugEnd = void 0;
  _debugProcess = void 0;
  _fatalException = void 0;
  _getActiveHandles = void 0;
  _getActiveRequests = void 0;
  _kill = void 0;
  _preload_modules = void 0;
  _rawDebug = void 0;
  _startProfilerIdleNotifier = void 0;
  _stopProfilerIdleNotifier = void 0;
  _tickCallback = void 0;
  _disconnect = void 0;
  _handleQueue = void 0;
  _pendingMessage = void 0;
  _channel = void 0;
  _send = void 0;
  _linkedBinding = void 0;
};

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/@cloudflare/unenv-preset/dist/runtime/node/process.mjs
var globalProcess = globalThis["process"];
var getBuiltinModule = globalProcess.getBuiltinModule;
var workerdProcess = getBuiltinModule("node:process");
var unenvProcess = new Process({
  env: globalProcess.env,
  hrtime,
  // `nextTick` is available from workerd process v1
  nextTick: workerdProcess.nextTick
});
var { exit, features, platform } = workerdProcess;
var {
  _channel,
  _debugEnd,
  _debugProcess,
  _disconnect,
  _events,
  _eventsCount,
  _exiting,
  _fatalException,
  _getActiveHandles,
  _getActiveRequests,
  _handleQueue,
  _kill,
  _linkedBinding,
  _maxListeners,
  _pendingMessage,
  _preload_modules,
  _rawDebug,
  _send,
  _startProfilerIdleNotifier,
  _stopProfilerIdleNotifier,
  _tickCallback,
  abort,
  addListener,
  allowedNodeEnvironmentFlags,
  arch,
  argv,
  argv0,
  assert: assert2,
  availableMemory,
  binding,
  channel,
  chdir,
  config,
  connected,
  constrainedMemory,
  cpuUsage,
  cwd,
  debugPort,
  disconnect,
  dlopen,
  domain,
  emit,
  emitWarning,
  env,
  eventNames,
  execArgv,
  execPath,
  exitCode,
  finalization,
  getActiveResourcesInfo,
  getegid,
  geteuid,
  getgid,
  getgroups,
  getMaxListeners,
  getuid,
  hasUncaughtExceptionCaptureCallback,
  hrtime: hrtime3,
  initgroups,
  kill,
  listenerCount,
  listeners,
  loadEnvFile,
  mainModule,
  memoryUsage,
  moduleLoadList,
  nextTick,
  off,
  on,
  once,
  openStdin,
  permission,
  pid,
  ppid,
  prependListener,
  prependOnceListener,
  rawListeners,
  reallyExit,
  ref,
  release,
  removeAllListeners,
  removeListener,
  report,
  resourceUsage,
  send,
  setegid,
  seteuid,
  setgid,
  setgroups,
  setMaxListeners,
  setSourceMapsEnabled,
  setuid,
  setUncaughtExceptionCaptureCallback,
  sourceMapsEnabled,
  stderr,
  stdin,
  stdout,
  throwDeprecation,
  title,
  traceDeprecation,
  umask,
  unref,
  uptime,
  version,
  versions
} = unenvProcess;
var _process = {
  abort,
  addListener,
  allowedNodeEnvironmentFlags,
  hasUncaughtExceptionCaptureCallback,
  setUncaughtExceptionCaptureCallback,
  loadEnvFile,
  sourceMapsEnabled,
  arch,
  argv,
  argv0,
  chdir,
  config,
  connected,
  constrainedMemory,
  availableMemory,
  cpuUsage,
  cwd,
  debugPort,
  dlopen,
  disconnect,
  emit,
  emitWarning,
  env,
  eventNames,
  execArgv,
  execPath,
  exit,
  finalization,
  features,
  getBuiltinModule,
  getActiveResourcesInfo,
  getMaxListeners,
  hrtime: hrtime3,
  kill,
  listeners,
  listenerCount,
  memoryUsage,
  nextTick,
  on,
  off,
  once,
  pid,
  platform,
  ppid,
  prependListener,
  prependOnceListener,
  rawListeners,
  release,
  removeAllListeners,
  removeListener,
  report,
  resourceUsage,
  setMaxListeners,
  setSourceMapsEnabled,
  stderr,
  stdin,
  stdout,
  title,
  throwDeprecation,
  traceDeprecation,
  umask,
  uptime,
  version,
  versions,
  // @ts-expect-error old API
  domain,
  initgroups,
  moduleLoadList,
  reallyExit,
  openStdin,
  assert: assert2,
  binding,
  send,
  exitCode,
  channel,
  getegid,
  geteuid,
  getgid,
  getgroups,
  getuid,
  setegid,
  seteuid,
  setgid,
  setgroups,
  setuid,
  permission,
  mainModule,
  _events,
  _eventsCount,
  _exiting,
  _maxListeners,
  _debugEnd,
  _debugProcess,
  _fatalException,
  _getActiveHandles,
  _getActiveRequests,
  _kill,
  _preload_modules,
  _rawDebug,
  _startProfilerIdleNotifier,
  _stopProfilerIdleNotifier,
  _tickCallback,
  _disconnect,
  _handleQueue,
  _pendingMessage,
  _channel,
  _send,
  _linkedBinding
};
var process_default = _process;

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/wrangler/_virtual_unenv_global_polyfill-@cloudflare-unenv-preset-node-process
globalThis.process = process_default;

// ../src/lib/http.ts
function segments(path) {
  const out = path.split("/");
  if (out[0] === "") out.shift();
  if (out.length && out[out.length - 1] === "") out.pop();
  return out;
}
__name(segments, "segments");
function safeDecode(s) {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}
__name(safeDecode, "safeDecode");
function compile(pattern) {
  const parts = segments(pattern);
  return (path) => {
    const segs = segments(path);
    if (segs.length !== parts.length) return null;
    const params = {};
    for (let i = 0; i < parts.length; i++) {
      const p = parts[i];
      if (p.startsWith(":")) {
        if (!segs[i]) return null;
        params[p.slice(1)] = safeDecode(segs[i]);
      } else if (p !== segs[i]) {
        return null;
      }
    }
    return params;
  };
}
__name(compile, "compile");
var Router = class {
  static {
    __name(this, "Router");
  }
  routes = [];
  add(method, pattern, handler) {
    this.routes.push({ method, match: compile(pattern), handler });
    return this;
  }
  /**
   * 找出匹配的处理函数。
   *
   * ⚠️ 路径匹配与**方法不匹配**必须报出不同的错：
   *  · 路径不存在 → `not_found`
   *  · 路径存在但方法不对 → `method_not_allowed`，并带 `allow` 列出该路径接受的方法
   * 只回 404 的话，「GET 写成了 POST」这种最常见的手误会伪装成「服务端没这个接口」，
   * 排查方向完全跑偏（约定 52 记的就是这类：404 让人以为域名/前缀错）。
   *
   * 返回**可判别联合**而不是裸的 `404 | 405` 数字：数字当返回值时 TypeScript
   * 无法收窄，调用方只能 `as` 强转或先 `typeof` 判断，写错一次就是运行期才炸。
   */
  resolve(method, path) {
    const pathMatched = /* @__PURE__ */ new Set();
    for (const r2 of this.routes) {
      const params = r2.match(path);
      if (params === null) continue;
      pathMatched.add(r2.method);
      if (r2.method === method) return { kind: "ok", handler: r2.handler, params };
    }
    if (pathMatched.size > 0) {
      const allow = [...pathMatched];
      return { kind: "method_not_allowed", allow };
    }
    return { kind: "not_found" };
  }
};
function json(data, status = 200, headers = {}) {
  return new Response(JSON.stringify(data), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      // 账号/配额类响应绝不能被任何中间层缓存
      "cache-control": "no-store",
      ...headers
    }
  });
}
__name(json, "json");
function fail(status, code, message, extra = {}, headers = {}) {
  return json({ error: code, message, ...extra }, status, headers);
}
__name(fail, "fail");
var unauthorized = /* @__PURE__ */ __name(() => fail(401, "unauthorized", "\u672A\u767B\u5F55\u6216\u4F1A\u8BDD\u5DF2\u5931\u6548"), "unauthorized");
async function readJson(req) {
  try {
    const v = await req.json();
    return v && typeof v === "object" ? v : {};
  } catch {
    return {};
  }
}
__name(readJson, "readJson");
function timingSafeEqual(a, b) {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}
__name(timingSafeEqual, "timingSafeEqual");
async function currentUser(env2, token) {
  if (!token) return null;
  const row = await env2.DB.prepare(
    `SELECT u.id, u.github_id, u.username, u.email, u.avatar, u.role,
            u.is_developer, u.developer_status, u.invite_redeemed
       FROM sessions s
       JOIN users u ON u.id = s.user_id
      WHERE s.token = ?1 AND s.expires_at > ?2`
  ).bind(token, Date.now()).first();
  return row ?? null;
}
__name(currentUser, "currentUser");

// ../src/lib/paths.ts
var OPENAI_COMPAT = {
  post_chat_completions: "/v1/chat/completions"
};

// ../src/lib/ids.ts
var randomId = /* @__PURE__ */ __name(() => crypto.randomUUID(), "randomId");

// ../src/routes/github.ts
var GH_OAUTH = "https://github.com";
var GH_API = "https://api.github.com";
var GH_FORM_HEADERS = {
  "content-type": "application/x-www-form-urlencoded",
  Accept: "application/json"
};
var POLL_TTL_MS = 15 * 6e4;
async function takePoll(ctx, pollId) {
  const row = await ctx.env.DB.prepare(
    `SELECT device_code, interval, expires_at FROM device_polls WHERE poll_id = ?1`
  ).bind(pollId).first();
  if (!row) return null;
  if (row.expires_at < Date.now()) {
    await ctx.env.DB.prepare(`DELETE FROM device_polls WHERE poll_id = ?1`).bind(pollId).run();
    return null;
  }
  return row;
}
__name(takePoll, "takePoll");
function dropPoll(ctx, pollId) {
  return ctx.env.DB.prepare(`DELETE FROM device_polls WHERE poll_id = ?1`).bind(pollId).run();
}
__name(dropPoll, "dropPoll");
async function ghFetch(url, init = {}) {
  const headers = new Headers(init.headers);
  headers.set("user-agent", "m-hub-server");
  headers.set("accept", "application/json");
  try {
    return await fetch(url, { ...init, headers });
  } catch (e) {
    throw new GitHubUnreachable(String(e));
  }
}
__name(ghFetch, "ghFetch");
var GitHubUnreachable = class extends Error {
  static {
    __name(this, "GitHubUnreachable");
  }
};
var unreachable = /* @__PURE__ */ __name((e) => fail(
  502,
  "github_unavailable",
  `\u670D\u52A1\u7AEF\u8FDE\u63A5 GitHub \u5931\u8D25\uFF1A${e}\u3002\u8FD9\u4E0E\u4F60\u7684\u7F51\u7EDC\u65E0\u5173\uFF0C\u8BF7\u7A0D\u540E\u91CD\u8BD5\u3002`
), "unreachable");
async function deviceStart(ctx) {
  const env2 = ctx.env;
  if (!env2.GITHUB_CLIENT_ID) {
    return fail(500, "server_misconfigured", "\u670D\u52A1\u7AEF\u672A\u914D\u7F6E GITHUB_CLIENT_ID");
  }
  const now = Date.now();
  await env2.DB.prepare(`DELETE FROM device_polls WHERE expires_at < ?1`).bind(now).run();
  let res;
  try {
    res = await ghFetch(`${GH_OAUTH}/login/device/code`, {
      method: "POST",
      // ⚠️ `Accept: application/json` **必须**带。
      //   缺它时 GitHub 的设备码端点返回的是 HTML / 纯文本（实测「Not Found」裸串），
      //   而下面 `res.json()` 会直接抛 —— 抛出的又是别的错误，故障现场离原因很远。
      headers: GH_FORM_HEADERS,
      body: new URLSearchParams({
        client_id: env2.GITHUB_CLIENT_ID,
        // 只读身份。**不要**加 repo / workflow 等任何写权限：用户看到的是这个 scope。
        scope: "read:user user:email"
      })
    });
  } catch (e) {
    return unreachable(String(e));
  }
  if (!res.ok) {
    return fail(502, "github_unavailable", `GitHub \u62D2\u7EDD\u7B7E\u53D1\u8BBE\u5907\u7801\uFF08HTTP ${res.status}\uFF09`);
  }
  const d = await res.json();
  const pollId = randomId();
  const expiresAt = now + Math.min(d.expires_in ?? 900, POLL_TTL_MS / 1e3) * 1e3;
  await env2.DB.prepare(
    `INSERT INTO device_polls (poll_id, device_code, interval, created_at, expires_at)
     VALUES (?1, ?2, ?3, ?4, ?5)`
  ).bind(pollId, d.device_code, d.interval ?? 5, now, expiresAt).run();
  return json({
    poll_id: pollId,
    user_code: d.user_code,
    verification_uri: d.verification_uri,
    interval: d.interval ?? 5,
    expires_in: d.expires_in ?? 900
  });
}
__name(deviceStart, "deviceStart");
async function devicePoll(ctx) {
  const env2 = ctx.env;
  const body = await readJson(ctx.req);
  const pollId = typeof body.poll_id === "string" ? body.poll_id : "";
  if (!pollId) return fail(400, "bad_request", "\u7F3A\u5C11 poll_id");
  const entry = await takePoll(ctx, pollId);
  if (!entry) {
    return fail(
      410,
      "poll_gone",
      "\u767B\u5F55\u4F1A\u8BDD\u5DF2\u5931\u6548\uFF08\u7B49\u5F85\u8FC7\u4E45\uFF09\uFF0C\u8BF7\u91CD\u65B0\u53D1\u8D77\u767B\u5F55\u3002"
    );
  }
  let res;
  try {
    res = await ghFetch(`${GH_OAUTH}/login/oauth/access_token`, {
      method: "POST",
      // ⚠️ `Accept: application/json` **必须**带。
      //   缺它时 GitHub 的设备码端点返回的是 HTML / 纯文本（实测「Not Found」裸串），
      //   而下面 `res.json()` 会直接抛 —— 抛出的又是别的错误，故障现场离原因很远。
      headers: GH_FORM_HEADERS,
      body: new URLSearchParams({
        client_id: env2.GITHUB_CLIENT_ID ?? "",
        device_code: entry.device_code,
        grant_type: "urn:ietf:params:oauth:grant-type:device_code"
      })
    });
  } catch (e) {
    return unreachable(String(e));
  }
  const d = await res.json();
  if (d.error) {
    if (d.error === "authorization_pending") {
      return json({ status: "pending", interval: entry.interval });
    }
    if (d.error === "slow_down") {
      const next = entry.interval + 5;
      await ctx.env.DB.prepare(`UPDATE device_polls SET interval = ?1 WHERE poll_id = ?2`).bind(next, pollId).run();
      return json({ status: "pending", interval: next });
    }
    await dropPoll(ctx, pollId);
    return fail(400, "github_denied", githubErrorText(d.error));
  }
  if (!d.access_token) {
    return fail(502, "github_unavailable", "GitHub \u672A\u8FD4\u56DE access_token");
  }
  await dropPoll(ctx, pollId);
  let ghUser;
  try {
    const r2 = await ghFetch(`${GH_API}/user`, {
      headers: { authorization: `Bearer ${d.access_token}` }
    });
    if (!r2.ok) {
      return fail(502, "github_unavailable", `\u8BFB\u53D6 GitHub \u7528\u6237\u4FE1\u606F\u5931\u8D25\uFF08HTTP ${r2.status}\uFF09`);
    }
    ghUser = await r2.json();
  } catch (e) {
    return unreachable(String(e));
  }
  const session = await upsertUserAndSession(env2, ghUser, ctx.req.headers.get("user-agent"));
  return json({ status: "ok", token: session.token, user: session.user });
}
__name(devicePoll, "devicePoll");
function githubErrorText(code) {
  switch (code) {
    case "expired_token":
    case "code_expired":
      return "\u8BBE\u5907\u7801\u5DF2\u8FC7\u671F\uFF0C\u8BF7\u91CD\u65B0\u53D1\u8D77\u767B\u5F55\u3002";
    case "access_denied":
      return "\u4F60\u5DF2\u62D2\u7EDD\u6388\u6743\u3002";
    case "incorrect_device_code":
      return "\u8BBE\u5907\u7801\u65E0\u6548\uFF0C\u8BF7\u91CD\u65B0\u53D1\u8D77\u767B\u5F55\u3002";
    default:
      return `GitHub \u62D2\u7EDD\u4E86\u672C\u6B21\u6388\u6743\uFF08${code}\uFF09\uFF0C\u8BF7\u91CD\u65B0\u53D1\u8D77\u767B\u5F55\u3002`;
  }
}
__name(githubErrorText, "githubErrorText");
async function upsertUserAndSession(env2, gh, ua) {
  const githubId = String(gh.id);
  await env2.DB.prepare(
    `INSERT INTO users (github_id, username, email, avatar)
     VALUES (?1, ?2, ?3, ?4)
     ON CONFLICT(github_id) DO UPDATE SET
       username = excluded.username,
       email    = COALESCE(excluded.email, users.email),
       avatar   = excluded.avatar`
  ).bind(githubId, gh.login, gh.email, gh.avatar_url).run();
  const user = await env2.DB.prepare(
    `SELECT id, github_id, username, email, avatar, role, is_developer, developer_status, invite_redeemed
       FROM users WHERE github_id = ?1`
  ).bind(githubId).first();
  if (!user) throw new Error("\u7528\u6237\u5199\u5165\u540E\u8BFB\u4E0D\u56DE\u6765");
  const token = `mhub_${randomId().replace(/-/g, "")}${randomId().replace(/-/g, "")}`;
  const expiresAt = Date.now() + 30 * 24 * 36e5;
  await env2.DB.prepare(
    `INSERT INTO sessions (token, user_id, device, expires_at) VALUES (?1, ?2, ?3, ?4)`
  ).bind(token, user.id, deviceLabel(ua), expiresAt).run();
  return { token, user };
}
__name(upsertUserAndSession, "upsertUserAndSession");
function deviceLabel(ua) {
  if (!ua) return "\u672A\u77E5\u8BBE\u5907";
  const s = ua.toLowerCase();
  if (s.includes("windows")) return "Windows";
  if (s.includes("android")) return "Android";
  if (s.includes("iphone")) return "iPhone";
  if (s.includes("ipad")) return "iPad";
  if (s.includes("mac os") || s.includes("macintosh")) return "macOS";
  if (s.includes("linux")) return "Linux";
  return ua.slice(0, 40);
}
__name(deviceLabel, "deviceLabel");

// ../src/lib/developerGate.ts
function canApplyDeveloper(u) {
  return !!u.invite_redeemed && u.developer_status === "none";
}
__name(canApplyDeveloper, "canApplyDeveloper");

// ../src/routes/me.ts
async function me(ctx) {
  const user = await currentUser(ctx.env, ctx.token);
  if (!user) return unauthorized();
  return json(publicUser(user, ctx.env));
}
__name(me, "me");
function publicUser(u, env2) {
  const available = !!env2?.OPENAI_API_KEY;
  return {
    username: u.username,
    email: u.email,
    avatar: u.avatar,
    role: u.role,
    is_developer: !!u.is_developer,
    developer_status: u.developer_status,
    invite_redeemed: !!u.invite_redeemed,
    // 复用**唯一**判定处（submissions.ts::canApplyDeveloper）——
    // 两处各算一份时，坏掉的那份会被用，且毫无报错（见该函数的注释）。
    can_apply_developer: canApplyDeveloper(u),
    platform_ai_available: available,
    platform_ai_reason: available ? "" : "\u5E73\u53F0 AI \u672A\u914D\u7F6E\u4E0A\u6E38 Key\u3002\u53EF\u6539\u7528\u300C\u8BBE\u7F6E \u2192 AI \u52A9\u624B\u300D\u91CC\u7684\u81EA\u5907\u4F9B\u5E94\u5546\uFF0C\u4E0D\u5F71\u54CD\u5176\u5B83\u529F\u80FD\u3002"
  };
}
__name(publicUser, "publicUser");
async function redeem(ctx) {
  const user = await currentUser(ctx.env, ctx.token);
  if (!user) return unauthorized();
  const body = await readJson(ctx.req);
  const code = typeof body.code === "string" ? body.code.trim() : "";
  if (!code) return fail(400, "bad_request", "\u7F3A\u5C11\u5151\u6362\u7801");
  if (!ctx.env.INVITE_CODE) {
    return fail(500, "server_misconfigured", "\u670D\u52A1\u7AEF\u672A\u914D\u7F6E\u9080\u8BF7\u7801");
  }
  if (code !== ctx.env.INVITE_CODE) {
    return fail(400, "invalid_code", "\u5151\u6362\u7801\u65E0\u6548");
  }
  if (user.invite_redeemed) {
    return fail(409, "already_redeemed", "\u8BE5\u8D26\u53F7\u5DF2\u5151\u6362\u8FC7");
  }
  await ctx.env.DB.prepare(`UPDATE users SET invite_redeemed = 1 WHERE id = ?1`).bind(user.id).run();
  return json({ ok: true, invite_redeemed: true });
}
__name(redeem, "redeem");
async function deviceTokens(ctx) {
  const user = await currentUser(ctx.env, ctx.token);
  if (!user) return unauthorized();
  const rows = await ctx.env.DB.prepare(
    `SELECT token, device, created_at AS createdAt, expires_at AS expiresAt
       FROM sessions WHERE user_id = ?1 ORDER BY created_at DESC`
  ).bind(user.id).all();
  const now = Date.now();
  return json({
    items: (rows.results ?? []).map((r2) => ({
      // token 只给前 8 位：够用户认出是哪台设备，又不足以当凭证用
      id: r2.token.slice(0, 8),
      device: r2.device,
      created_at: r2.createdAt,
      expires_at: r2.expiresAt,
      expired: r2.expiresAt <= now,
      current: r2.token === ctx.token
    }))
  });
}
__name(deviceTokens, "deviceTokens");
async function deviceRevoke(ctx, params) {
  const user = await currentUser(ctx.env, ctx.token);
  if (!user) return unauthorized();
  const r2 = await ctx.env.DB.prepare(
    `DELETE FROM sessions WHERE user_id = ?1 AND token LIKE ?2`
  ).bind(user.id, `${params.id}%`).run();
  if (!r2.meta.changes) return fail(404, "not_found", "\u627E\u4E0D\u5230\u8BE5\u8BBE\u5907");
  return json({ ok: true });
}
__name(deviceRevoke, "deviceRevoke");

// ../src/routes/ai.ts
var UPSTREAM_BASE = "https://api.openai.com";
var DAILY_GRANT = 20;
async function models(ctx) {
  const user = await currentUser(ctx.env, ctx.token);
  if (!user) return unauthorized();
  if (!user.invite_redeemed) {
    return fail(403, "invite_required", "\u9700\u5148\u5151\u6362\u9080\u8BF7\u7801\u624D\u80FD\u4F7F\u7528\u5E73\u53F0\u989D\u5EA6");
  }
  if (!ctx.env.OPENAI_API_KEY) {
    return json({
      available: false,
      reason: "\u5E73\u53F0 AI \u672A\u914D\u7F6E\u4E0A\u6E38 Key\uFF0C\u65E0\u6CD5\u4F7F\u7528\u3002\u4F60\u7684\u8D39\u7528\u4E0D\u8BE5\u7531\u5E73\u53F0\u627F\u62C5 \u2014\u2014 \u8BF7\u6539\u7528\u81EA\u5907\u4F9B\u5E94\u5546\u3002",
      models: [],
      quota: { granted: 0, used: 0, remaining: 0, day: "" }
    });
  }
  return json({
    available: true,
    models: platformModelIds().map((id) => ({ id: `platform:${id}`, name: `m-hub \u5E73\u53F0 \xB7 ${id}` })),
    quota: await todayQuota(ctx, user.id)
  });
}
__name(models, "models");
function platformModelIds() {
  return (process.env.MHUB_PLATFORM_MODELS || "").split(",").map((s) => s.trim()).filter(Boolean);
}
__name(platformModelIds, "platformModelIds");
async function chatCompletions(ctx) {
  const user = await currentUser(ctx.env, ctx.token);
  if (!user) return unauthorized();
  if (!user.invite_redeemed) {
    return fail(403, "invite_required", "\u9700\u5148\u5151\u6362\u9080\u8BF7\u7801\u624D\u80FD\u4F7F\u7528\u5E73\u53F0\u989D\u5EA6");
  }
  if (!ctx.env.OPENAI_API_KEY) {
    return fail(
      503,
      "platform_unavailable",
      "\u5E73\u53F0 AI \u672A\u914D\u7F6E\u4E0A\u6E38 Key\uFF0C\u65E0\u6CD5\u4F7F\u7528\u3002\u8BF7\u6539\u7528\u81EA\u5907\u4F9B\u5E94\u5546\u3002"
    );
  }
  const remaining = await consumeQuota(ctx, user.id);
  if (remaining <= 0) {
    return fail(429, "quota_exhausted", "\u4ECA\u65E5\u5E73\u53F0\u989D\u5EA6\u5DF2\u7528\u5B8C\uFF0C\u660E\u5929\u518D\u6765");
  }
  const body = await ctx.req.text();
  const upstream = await fetch(`${UPSTREAM_BASE}/v1/chat/completions`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${ctx.env.OPENAI_API_KEY ?? ""}`
    },
    body
  });
  if (!upstream.ok) {
    const text = await upstream.text().catch(() => "");
    return fail(
      upstream.status === 429 ? 429 : 502,
      "upstream_error",
      `\u4E0A\u6E38\u8FD4\u56DE ${upstream.status}\uFF1A${text.slice(0, 200)}`
    );
  }
  return new Response(upstream.body, {
    status: 200,
    headers: {
      "content-type": upstream.headers.get("content-type") ?? "text/event-stream",
      "cache-control": "no-store",
      "x-accel-buffering": "no"
      // 反代/浏览器侧的缓冲关掉
    }
  });
}
__name(chatCompletions, "chatCompletions");
async function todayQuota(ctx, userId) {
  const day = (/* @__PURE__ */ new Date()).toISOString().slice(0, 10);
  const r2 = await ctx.env.DB.prepare(
    `SELECT granted, used FROM ai_quota WHERE user_id = ?1 AND day = ?2`
  ).bind(userId, day).first();
  const granted = r2?.granted ?? DAILY_GRANT;
  const used = r2?.used ?? 0;
  if (!r2) {
    await ctx.env.DB.prepare(
      // ⚠️ `ON CONFLICT DO NOTHING` 是 PostgreSQL 版的「INSERT OR IGNORE」。
      // 数据层会把 `INSERT OR IGNORE` 改写成 `INSERT`，但**补不出 ON CONFLICT**
      // （那需要知道冲突目标），所以子句必须显式写在这里。
      // 没有它的话 PG 会直接语法报错 —— 属「应用自身问题」，看日志尾即可定位。
      `INSERT INTO ai_quota (user_id, day, granted, used) VALUES (?1, ?2, ?3, 0)
       ON CONFLICT (user_id, day) DO NOTHING`
    ).bind(userId, day, DAILY_GRANT).run();
  }
  return { granted, used, remaining: Math.max(0, granted - used), day };
}
__name(todayQuota, "todayQuota");
async function consumeQuota(ctx, userId) {
  const day = (/* @__PURE__ */ new Date()).toISOString().slice(0, 10);
  await ctx.env.DB.prepare(
    // 同上：ON CONFLICT 子句必须显式写（PG 无「OR IGNORE」等价物）
    `INSERT INTO ai_quota (user_id, day, granted, used) VALUES (?1, ?2, ?3, 0)
     ON CONFLICT (user_id, day) DO NOTHING`
  ).bind(userId, day, DAILY_GRANT).run();
  const r2 = await ctx.env.DB.prepare(
    `UPDATE ai_quota SET used = used + 1
      WHERE user_id = ?1 AND day = ?2 AND used < granted
      RETURNING granted - used AS remaining`
  ).bind(userId, day).first();
  return r2?.remaining ?? 0;
}
__name(consumeQuota, "consumeQuota");

// ../src/routes/email.ts
var CODE_TTL_MS = 10 * 6e4;
var RESEND_COOLDOWN_MS = 6e4;
var MAX_PER_HOUR = 5;
var codes = /* @__PURE__ */ new Map();
var sentAt = /* @__PURE__ */ new Map();
function sweep(now) {
  for (const [k, v] of codes) if (now - v.at > CODE_TTL_MS) codes.delete(k);
  for (const [k, arr] of sentAt) {
    const kept = arr.filter((t) => now - t < 36e5);
    if (kept.length) sentAt.set(k, kept);
    else sentAt.delete(k);
  }
}
__name(sweep, "sweep");
async function send2(ctx) {
  const { env: env2 } = ctx;
  const body = await readJson(ctx.req);
  const email = String(body.email ?? "").trim().toLowerCase();
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
    return fail(400, "bad_request", "\u90AE\u7BB1\u683C\u5F0F\u4E0D\u6B63\u786E");
  }
  if (!env2.RESEND_API_KEY || !env2.EMAIL_FROM) {
    return fail(501, "not_configured", "\u670D\u52A1\u7AEF\u672A\u542F\u7528\u90AE\u7BB1\u767B\u5F55\uFF0C\u8BF7\u7528 GitHub \u767B\u5F55");
  }
  const now = Date.now();
  sweep(now);
  const inHour = (sentAt.get(email) ?? []).filter((t) => now - t < 36e5);
  if (inHour.length >= MAX_PER_HOUR) {
    const waitMin = Math.ceil((36e5 - (now - inHour[0])) / 6e4);
    return fail(
      429,
      "rate_limited",
      `\u540C\u4E00\u90AE\u7BB1\u6BCF\u5C0F\u65F6\u6700\u591A ${MAX_PER_HOUR} \u5C01\uFF0C\u8BF7 ${waitMin} \u5206\u949F\u540E\u518D\u8BD5\u3002\uFF08\u5DF2\u53D1 ${inHour.length} \u5C01\uFF09`
    );
  }
  const prev = codes.get(email);
  if (prev && now - prev.at < RESEND_COOLDOWN_MS) {
    const wait = Math.ceil((RESEND_COOLDOWN_MS - (now - prev.at)) / 1e3);
    return fail(
      429,
      "rate_limited",
      `\u53D1\u9001\u8FC7\u4E8E\u9891\u7E41\uFF0C\u8BF7 ${wait} \u79D2\u540E\u518D\u8BD5\uFF08\u540C\u4E00\u90AE\u7BB1 60 \u79D2\u53EA\u80FD\u53D1\u4E00\u5C01\uFF0C\u6BCF\u5C0F\u65F6\u6700\u591A ${MAX_PER_HOUR} \u5C01\uFF09`
    );
  }
  const code = String(Math.floor(1e5 + Math.random() * 9e5));
  codes.set(email, { code, at: now, tries: 0 });
  const res = await fetch("https://api.resend.com/emails", {
    method: "POST",
    headers: {
      authorization: `Bearer ${env2.RESEND_API_KEY}`,
      "content-type": "application/json"
    },
    body: JSON.stringify({
      from: env2.EMAIL_FROM,
      to: [email],
      subject: "m-hub \u767B\u5F55\u9A8C\u8BC1\u7801",
      text: `\u4F60\u7684\u9A8C\u8BC1\u7801\u662F ${code}\uFF0C10 \u5206\u949F\u5185\u6709\u6548\u3002\u5982\u679C\u8FD9\u4E0D\u662F\u4F60\u672C\u4EBA\u7684\u64CD\u4F5C\uFF0C\u5FFD\u7565\u8FD9\u5C01\u4FE1\u5373\u53EF\u3002`
    })
  });
  if (!res.ok) {
    codes.delete(email);
    return fail(502, "email_send_failed", `\u53D1\u4FE1\u5931\u8D25\uFF08HTTP ${res.status}\uFF09\uFF0C\u8BF7\u7A0D\u540E\u91CD\u8BD5`);
  }
  inHour.push(now);
  sentAt.set(email, inHour);
  return json({ ok: true, cooldown_ms: RESEND_COOLDOWN_MS, expires_in: CODE_TTL_MS / 1e3 });
}
__name(send2, "send");
async function verify(ctx) {
  const env2 = ctx.env;
  const body = await readJson(ctx.req);
  const email = String(body.email ?? "").trim().toLowerCase();
  const code = String(body.code ?? "").trim();
  if (!email || !code) return fail(400, "bad_request", "\u7F3A\u5C11\u90AE\u7BB1\u6216\u9A8C\u8BC1\u7801");
  const entry = codes.get(email);
  if (!entry) return fail(400, "code_invalid", "\u9A8C\u8BC1\u7801\u65E0\u6548\u6216\u5DF2\u8FC7\u671F");
  if (Date.now() - entry.at > CODE_TTL_MS) {
    codes.delete(email);
    return fail(400, "code_expired", "\u9A8C\u8BC1\u7801\u5DF2\u8FC7\u671F\uFF0C\u8BF7\u91CD\u65B0\u83B7\u53D6");
  }
  entry.tries += 1;
  if (entry.tries > 5) {
    codes.delete(email);
    return fail(429, "too_many_tries", "\u5C1D\u8BD5\u6B21\u6570\u8FC7\u591A\uFF0C\u8BF7\u91CD\u65B0\u83B7\u53D6\u9A8C\u8BC1\u7801");
  }
  if (entry.code !== code) return fail(400, "code_invalid", "\u9A8C\u8BC1\u7801\u4E0D\u6B63\u786E");
  codes.delete(email);
  const githubId = null;
  const username = email.split("@")[0];
  await env2.DB.prepare(
    `INSERT INTO users (github_id, username, email) VALUES (?1, ?2, ?3)
     ON CONFLICT(email) DO UPDATE SET username = excluded.username`
  ).bind(githubId, username, email).run();
  const user = await env2.DB.prepare(
    `SELECT id, github_id, username, email, avatar, role, is_developer, developer_status, invite_redeemed
       FROM users WHERE email = ?1`
  ).bind(email).first();
  if (!user) return fail(500, "server_error", "\u8D26\u53F7\u521B\u5EFA\u5931\u8D25");
  const token = `mhub_${randomId().replace(/-/g, "")}${randomId().replace(/-/g, "")}`;
  await env2.DB.prepare(
    `INSERT INTO sessions (token, user_id, device, expires_at) VALUES (?1, ?2, ?3, ?4)`
  ).bind(token, user.id, "\u90AE\u7BB1\u767B\u5F55", Date.now() + 30 * 24 * 36e5).run();
  return json({ status: "ok", token, user: {
    username: user.username,
    email: user.email,
    avatar: user.avatar,
    role: user.role,
    is_developer: !!user.is_developer,
    developer_status: user.developer_status,
    invite_redeemed: !!user.invite_redeemed
  } });
}
__name(verify, "verify");

// ../src/lib/pkgStore.ts
var CHUNK_SIZE = 128 * 1024;
function sliceChunks(bytes, size = CHUNK_SIZE) {
  const out = [];
  for (let off2 = 0; off2 < bytes.length; off2 += size) {
    out.push(bytes.subarray(off2, off2 + size));
  }
  return out;
}
__name(sliceChunks, "sliceChunks");
function joinChunks(chunks, total) {
  const out = new Uint8Array(total);
  let off2 = 0;
  for (const c of chunks) {
    if (off2 >= total) break;
    const n = Math.min(c.length, total - off2);
    out.set(c.subarray(0, n), off2);
    off2 += n;
  }
  return out;
}
__name(joinChunks, "joinChunks");
function toHex(bytes) {
  let s = "";
  for (const b of bytes) s += b.toString(16).padStart(2, "0");
  return s;
}
__name(toHex, "toHex");
async function sha256Hex(bytes) {
  const d = await crypto.subtle.digest("SHA-256", bytes);
  return toHex(new Uint8Array(d));
}
__name(sha256Hex, "sha256Hex");
async function putBlob(db, bytes) {
  const sha = await sha256Hex(bytes);
  await db.prepare(`DELETE FROM pkg_blobs WHERE sha256 = ?1`).bind(sha).run();
  const chunks = sliceChunks(bytes);
  for (let i = 0; i < chunks.length; i++) {
    await db.prepare(`INSERT INTO pkg_blobs (sha256, idx, data) VALUES (?1, ?2, ?3)`).bind(sha, i, chunks[i]).run();
  }
  return sha;
}
__name(putBlob, "putBlob");
async function getBlob(db, sha, total) {
  const rows = await db.prepare(`SELECT data FROM pkg_blobs WHERE sha256 = ?1 ORDER BY idx`).bind(sha).all();
  if (!rows.results || rows.results.length === 0) return null;
  const chunks = rows.results.map(
    (r2) => r2.data instanceof Uint8Array ? r2.data : new Uint8Array(r2.data)
  );
  return joinChunks(chunks, total);
}
__name(getBlob, "getBlob");

// ../src/lib/zipdir.ts
var EOCD_SIG = 101010256;
var ZIP64_EOCD_LOC_SIG = 117853008;
var EOCD64_SIG = 101075792;
var CD_SIG = 33639248;
function rd16(b, o) {
  return b[o] | b[o + 1] << 8;
}
__name(rd16, "rd16");
function rd32(b, o) {
  return (b[o] | b[o + 1] << 8 | b[o + 2] << 16) + b[o + 3] * 16777216;
}
__name(rd32, "rd32");
function listZipEntries(bytes) {
  const eocd = findEocd(bytes);
  let cdCount = rd16(bytes, eocd + 10);
  let cdOffset = rd32(bytes, eocd + 16);
  if (cdCount === 65535 || cdOffset === 4294967295) {
    const z64 = findZip64Eocd(bytes);
    if (z64 >= 0) {
      cdCount = Number(rd32(bytes, z64 + 32) | rd32(bytes, z64 + 36) * 4294967296);
    }
  }
  const out = [];
  let p = cdOffset;
  for (let i = 0; i < cdCount; i++) {
    if (p + 46 > bytes.length || rd32(bytes, p) !== CD_SIG) break;
    const flags2 = rd16(bytes, p + 8);
    const nameLen = rd16(bytes, p + 28);
    const extraLen = rd16(bytes, p + 30);
    const commentLen = rd16(bytes, p + 32);
    const nameStart = p + 46;
    if (nameStart + nameLen > bytes.length) break;
    let path = "";
    for (let k = 0; k < nameLen; k++) path += String.fromCharCode(bytes[nameStart + k]);
    out.push({
      path,
      size: rd32(bytes, p + 24),
      compressedSize: rd32(bytes, p + 20),
      // ⚠️ **压缩与否看 method（+10），不看 flags 的 bit 3**。
      //    bit 3（0x0008）是「数据描述符跟在数据后面」，与压缩无关 ——
      //    我第一版就是把它当压缩标志，导致「deflate 压缩的条目」断言失败。
      //    而一个**合法的** deflate zip 完全可能 flags=0（method=8 已说明一切），
      //    所以那个断言本来也不该那么写。
      method: rd16(bytes, p + 10),
      encrypted: (flags2 & 1) !== 0,
      localOffset: rd32(bytes, p + 42)
    });
    p = nameStart + nameLen + extraLen + commentLen;
  }
  if (out.length === 0 && cdCount > 0) {
    throw new Error("\u4E2D\u592E\u76EE\u5F55\u89E3\u6790\u4E3A\u7A7A\uFF08\u6587\u4EF6\u635F\u574F\u3001\u88AB\u622A\u65AD\uFF0C\u6216\u5E26\u524D\u7F6E\u5B57\u8282\u7684\u81EA\u89E3\u538B\u5305\uFF09");
  }
  return out;
}
__name(listZipEntries, "listZipEntries");
function findEocd(b) {
  const min = Math.max(0, b.length - 65557);
  for (let i = b.length - 22; i >= min; i--) {
    if (rd32(b, i) === EOCD_SIG) return i;
  }
  throw new Error("\u4E0D\u662F zip\uFF1A\u627E\u4E0D\u5230\u4E2D\u592E\u76EE\u5F55\u7ED3\u675F\u8BB0\u5F55\uFF08EOCD\uFF09");
}
__name(findEocd, "findEocd");
function findZip64Eocd(b) {
  const eocd = findEocd(b);
  const loc = eocd - 20;
  if (loc < 0 || rd32(b, loc) !== ZIP64_EOCD_LOC_SIG) return -1;
  const z64 = rd32(b, loc + 8);
  return z64 === EOCD64_SIG || rd32(b, z64) === EOCD64_SIG ? z64 : -1;
}
__name(findZip64Eocd, "findZip64Eocd");

// ../src/lib/inflate.ts
async function inflateRaw(data, maxOutputBytes) {
  const ds = new DecompressionStream("deflate-raw");
  const writer = ds.writable.getWriter();
  const feed = (async () => {
    await writer.write(data);
    await writer.close();
  })();
  feed.catch(() => {
  });
  const reader = ds.readable.getReader();
  const chunks = [];
  let total = 0;
  for (; ; ) {
    const { done, value } = await reader.read();
    if (done) break;
    if (!value) continue;
    total += value.byteLength;
    if (total > maxOutputBytes) {
      await reader.cancel().catch(() => {
      });
      await feed.catch(() => {
      });
      throw new Error(`\u89E3\u538B\u540E\u8D85\u8FC7 ${maxOutputBytes} \u5B57\u8282\u4E0A\u9650\uFF08\u53EF\u80FD\u662F zip bomb\uFF09`);
    }
    chunks.push(value);
  }
  const out = new Uint8Array(total);
  let off2 = 0;
  for (const c of chunks) {
    out.set(c, off2);
    off2 += c.byteLength;
  }
  return out;
}
__name(inflateRaw, "inflateRaw");

// ../src/lib/zipread.ts
var LFH_SIG = 67324752;
function rd162(b, o) {
  return b[o] | b[o + 1] << 8;
}
__name(rd162, "rd16");
function rd322(b, o) {
  return (b[o] | b[o + 1] << 8 | b[o + 2] << 16) + b[o + 3] * 16777216;
}
__name(rd322, "rd32");
async function readZipEntry(bytes, entries, path, maxBytes) {
  const e = entries.find((x) => x.path === path);
  if (!e) return null;
  if (e.encrypted) throw new Error(`${path} \u662F\u52A0\u5BC6\u6761\u76EE\uFF0C\u65E0\u6CD5\u8BFB\u53D6`);
  const p = e.localOffset;
  if (p + 30 > bytes.length || rd322(bytes, p) !== LFH_SIG) {
    throw new Error(`${path} \u7684\u5C40\u90E8\u6587\u4EF6\u5934\u65E0\u6548\uFF08\u504F\u79FB ${p}\uFF09\u2014\u2014 zip \u53EF\u80FD\u635F\u574F\u6216\u6709\u524D\u7F6E\u5B57\u8282`);
  }
  const nameLen = rd162(bytes, p + 26);
  const extraLen = rd162(bytes, p + 28);
  const start = p + 30 + nameLen + extraLen;
  const end = start + e.compressedSize;
  if (end > bytes.length) throw new Error(`${path} \u7684\u6570\u636E\u8D85\u51FA\u6587\u4EF6\u672B\u5C3E\uFF08zip \u635F\u574F\uFF09`);
  const raw = bytes.subarray(start, end);
  if (e.method === 0) {
    if (raw.length > maxBytes) throw new Error(`${path} \u672A\u538B\u7F29\u5C31\u6709 ${raw.length} \u5B57\u8282\uFF0C\u8D85\u8FC7 ${maxBytes}`);
    return raw;
  }
  if (e.method !== 8) {
    throw new Error(`${path} \u4F7F\u7528\u4E86\u4E0D\u652F\u6301\u7684\u538B\u7F29\u65B9\u6CD5 ${e.method}\uFF08\u53EA\u652F\u6301 0=store / 8=deflate\uFF09`);
  }
  return inflateRaw(raw, maxBytes);
}
__name(readZipEntry, "readZipEntry");

// ../src/lib/gate.ts
var ALLOWED_EXT = /* @__PURE__ */ new Set([
  ".json",
  ".html",
  ".htm",
  ".css",
  ".js",
  ".mjs",
  ".cjs",
  ".txt",
  ".md",
  ".svg",
  ".png",
  ".jpg",
  ".jpeg",
  ".gif",
  ".webp",
  ".ico",
  ".woff",
  ".woff2",
  ".ttf",
  ".otf"
  // 约定 46：包内无扩展名的 LICENSE 在打包时会被改名成 LICENSE.txt，
  // 所以 .txt 覆盖得到；但**不要**因此放宽成「允许无扩展名」——
  // 那会让任意可执行文件（无扩展名的 sh/py）混进包里。
]);
var DENY_DIRS = ["node_modules/", ".git/", ".DS_Store"];
var MAX_MANIFEST_BYTES = 256 * 1024;
var ALLOWED_PERMISSIONS = /* @__PURE__ */ new Set([
  "storage",
  "config",
  "shared-storage",
  "network",
  "system",
  "clipboard",
  "open-url",
  "files",
  "notify",
  "window",
  "events",
  "theme"
]);
async function runGate(entries, readFile, manifestId) {
  const problems = [];
  const files = entries.filter((e) => !e.path.endsWith("/"));
  const enc = files.find((e) => e.encrypted);
  if (enc) problems.push(`\u5305\u5185\u6709\u52A0\u5BC6\u6761\u76EE\uFF0C\u65E0\u6CD5\u5BA1\u6838\uFF1A${enc.path}`);
  for (const e of files) {
    if (e.path.includes("..") || e.path.startsWith("/") || /^[A-Za-z]:/.test(e.path)) {
      problems.push(`\u5305\u5185\u8DEF\u5F84\u975E\u6CD5\uFF08\u53EF\u80FD\u8D8A\u6743\u5199\u5165\uFF09\uFF1A${e.path}`);
      break;
    }
  }
  for (const d of DENY_DIRS) {
    const hit = files.find((e) => e.path.includes(d));
    if (hit) {
      problems.push(`\u5305\u5185\u4E0D\u8BE5\u5305\u542B ${d}\uFF08\u7EA6\u5B9A 46 \u6253\u5305\u65F6\u5C31\u5E94\u6392\u9664\uFF09\uFF1A${hit.path}`);
      break;
    }
  }
  const bad = files.find((e) => {
    const base = e.path.slice(e.path.lastIndexOf("/") + 1);
    if (!base.includes(".")) return true;
    const i = base.lastIndexOf(".");
    return !ALLOWED_EXT.has(base.slice(i).toLowerCase());
  });
  if (bad) {
    problems.push(
      `\u5305\u5185\u6587\u4EF6\u6269\u5C55\u540D\u4E0D\u5728\u767D\u540D\u5355\u5185\uFF1A${bad.path}\u3002\u5141\u8BB8\uFF1A${[...ALLOWED_EXT].join(" ")}`
    );
  }
  const rootManifest = files.find((e) => e.path === "manifest.json");
  if (!rootManifest) {
    problems.push("\u5305\u6839\u7F3A\u5C11 manifest.json\uFF08\u7EA6\u5B9A 46\uFF1A\u5B83\u5FC5\u987B\u5728\u5305\u6839\uFF0C\u5426\u5219\u5BA2\u6237\u7AEF\u626B\u4E0D\u5230\u8FD9\u4E2A\u6269\u5C55\uFF09");
  } else if (rootManifest.size > MAX_MANIFEST_BYTES) {
    problems.push(`manifest.json \u8FC7\u5927\uFF08${rootManifest.size} \u5B57\u8282 > ${MAX_MANIFEST_BYTES}\uFF09`);
  }
  const totalSize = files.reduce((n, e) => n + e.size, 0);
  const huge = files.find((e) => e.size > 32 * 1024 * 1024);
  if (huge) problems.push(`\u5355\u4E2A\u6587\u4EF6\u8FC7\u5927\uFF1A${huge.path}\uFF08${(huge.size / 1048576).toFixed(1)}MB\uFF09`);
  let permissionsUsed = [];
  if (rootManifest && rootManifest.size <= MAX_MANIFEST_BYTES) {
    const raw = await readFile("manifest.json").catch(() => null);
    if (!raw) {
      problems.push("\u8BFB\u4E0D\u51FA manifest.json \u7684\u5185\u5BB9\uFF0C\u65E0\u6CD5\u6838\u5BF9\u6743\u9650\uFF08\u5305\u53EF\u80FD\u5DF2\u635F\u574F\uFF09");
    } else {
      let mf;
      try {
        mf = JSON.parse(new TextDecoder().decode(raw));
      } catch (e) {
        problems.push(`manifest.json \u4E0D\u662F\u5408\u6CD5 JSON\uFF1A${String(e).slice(0, 120)}`);
        return {
          ok: false,
          reason: problems[0],
          problems,
          permissionsUsed: [],
          fileCount: files.length,
          totalSize
        };
      }
      const decl = Array.isArray(mf.permissions) ? mf.permissions.filter((p) => typeof p === "string") : [];
      const over = decl.filter((p) => !ALLOWED_PERMISSIONS.has(p));
      if (over.length) {
        problems.push(`manifest \u7533\u8BF7\u4E86\u4E0D\u5B58\u5728\u7684\u6743\u9650\uFF1A${over.join(", ")}`);
      }
      permissionsUsed = decl;
      if (manifestId && typeof mf.id === "string" && mf.id !== manifestId) {
        problems.push(`manifest \u91CC\u7684 id\uFF08${mf.id}\uFF09\u4E0E\u63D0\u4EA4\u7684\u6269\u5C55 id\uFF08${manifestId}\uFF09\u4E0D\u4E00\u81F4`);
      }
    }
  }
  return {
    ok: problems.length === 0,
    reason: problems[0],
    problems,
    permissionsUsed,
    fileCount: files.length,
    totalSize
  };
}
__name(runGate, "runGate");

// ../src/routes/submissions.ts
var MAX_PACKAGE_BYTES = 64 * 1024 * 1024;
var MAX_MANIFEST_BYTES2 = 256 * 1024;
var OPEN_STATUSES = ["uploaded", "pending_review", "gate_failed"];
async function requireDeveloper(ctx) {
  const user = await currentUser(ctx.env, ctx.token);
  if (!user) return unauthorized();
  if (!user.is_developer) {
    return fail(403, "not_developer", "\u9700\u5148\u901A\u8FC7\u5F00\u53D1\u8005\u8BA4\u8BC1");
  }
  return user;
}
__name(requireDeveloper, "requireDeveloper");
async function apply(ctx) {
  const user = await currentUser(ctx.env, ctx.token);
  if (!user) return unauthorized();
  const body = await readJson(ctx.req);
  const reason = typeof body.reason === "string" ? body.reason.trim() : "";
  if (!reason) return fail(400, "bad_request", "\u8BF7\u586B\u5199\u7533\u8BF7\u7406\u7531");
  if (reason.length > 2e3) return fail(400, "bad_request", "\u7533\u8BF7\u7406\u7531\u8FC7\u957F");
  if (!user.invite_redeemed) {
    return fail(403, "invite_required", "\u9700\u5148\u5151\u6362\u9080\u8BF7\u7801");
  }
  if (user.developer_status === "approved") {
    return fail(409, "already_developer", "\u4F60\u5DF2\u662F\u5F00\u53D1\u8005");
  }
  try {
    await ctx.env.DB.prepare(
      `INSERT INTO dev_applications (user_id, reason) VALUES (?1, ?2)`
    ).bind(user.id, reason).run();
  } catch (e) {
    if (String(e).includes("UNIQUE")) {
      return fail(409, "already_pending", "\u4F60\u5DF2\u6709\u4E00\u4EFD\u7533\u8BF7\u5728\u5BA1\u6838\u4E2D");
    }
    throw e;
  }
  await ctx.env.DB.prepare(
    `UPDATE users SET developer_status = 'pending' WHERE id = ?1`
  ).bind(user.id).run();
  return json({ ok: true, status: "pending" });
}
__name(apply, "apply");
async function applyStatus(ctx) {
  const user = await currentUser(ctx.env, ctx.token);
  if (!user) return unauthorized();
  const row = await ctx.env.DB.prepare(
    `SELECT id, status, reason, review_note AS reviewNote, created_at AS createdAt
       FROM dev_applications WHERE user_id = ?1 ORDER BY created_at DESC LIMIT 1`
  ).bind(user.id).first();
  return json({
    status: user.developer_status,
    is_developer: !!user.is_developer,
    // 复用共享判定（见该函数的注释：这里曾与 me.ts 各算一份）
    can_apply_developer: canApplyDeveloper(user),
    application: row ?? null
  });
}
__name(applyStatus, "applyStatus");
async function submit(ctx) {
  const user = await requireDeveloper(ctx);
  if (user instanceof Response) return user;
  const form = await ctx.req.formData().catch(() => null);
  if (!form) return fail(400, "bad_request", "\u9700\u8981 multipart/form-data");
  const extId = String(form.get("ext_id") ?? "").trim();
  const version2 = String(form.get("version") ?? "").trim();
  if (!extId || !version2) return fail(400, "bad_request", "\u7F3A\u5C11 ext_id \u6216 version");
  if (!/^[a-z0-9][a-z0-9.-]*$/i.test(extId)) {
    return fail(400, "bad_request", "\u6269\u5C55 id \u683C\u5F0F\u4E0D\u5408\u6CD5");
  }
  if (!/^\d+\.\d+\.\d+$/.test(version2)) {
    return fail(400, "bad_request", "\u7248\u672C\u53F7\u5FC5\u987B\u662F x.y.z \u4E09\u6BB5\u7EAF\u6570\u5B57");
  }
  const pkgRaw = form.get("package");
  if (!(pkgRaw instanceof File)) return fail(400, "bad_request", "\u7F3A\u5C11\u6269\u5C55\u5305\u6587\u4EF6");
  const pkg = pkgRaw;
  const open = await ctx.env.DB.prepare(
    `SELECT id, version, status FROM submissions
      WHERE user_id = ?1 AND ext_id = ?2 AND status IN ('uploaded','pending_review','gate_failed')
      ORDER BY created_at DESC LIMIT 1`
  ).bind(user.id, extId).first();
  if (open) {
    return fail(409, "blocked_by_open_submission", `${open.id}`, {
      message: `\u8BE5\u6269\u5C55\u5DF2\u6709\u672A\u8D70\u5B8C\u6D41\u7A0B\u7684\u63D0\u4EA4\uFF1Av${open.version}\uFF08${open.status}\uFF09\u3002\u8BF7\u5148\u64A4\u56DE\u5B83\uFF0C\u6216\u7B49\u7ED3\u679C\u3002`,
      blocking: { id: open.id, version: open.version, status: open.status }
    });
  }
  const published = await ctx.env.DB.prepare(
    `SELECT version FROM submissions
      WHERE user_id = ?1 AND ext_id = ?2 AND status = 'published'
      ORDER BY created_at DESC LIMIT 1`
  ).bind(user.id, extId).first();
  if (published && compareSemver(version2, published.version) <= 0) {
    return fail(
      400,
      "version_not_incremented",
      `\u65B0\u7248\u672C\u5FC5\u987B\u4E25\u683C\u5927\u4E8E\u5DF2\u53D1\u5E03\u7248\u672C v${published.version}`
    );
  }
  const bytes = new Uint8Array(await pkg.arrayBuffer());
  if (bytes.byteLength === 0) return fail(400, "bad_request", "\u6269\u5C55\u5305\u662F\u7A7A\u7684");
  if (bytes.byteLength > MAX_PACKAGE_BYTES) {
    return fail(400, "package_too_large", `\u6269\u5C55\u5305\u8D85\u8FC7 ${MAX_PACKAGE_BYTES / 1048576}MB`);
  }
  const sha256 = await putBlob(ctx.env.DB, bytes);
  let pkgFiles = null;
  try {
    pkgFiles = listZipEntries(bytes);
  } catch (e) {
    console.warn("[m-hub] \u5305\u76EE\u5F55\u89E3\u6790\u5931\u8D25\uFF0C\u5173\u5361\u5C06\u65E0\u6CD5\u5224\u5B9A\u6761\u76EE:", String(e));
  }
  const gate = await runGate(
    pkgFiles ?? [],
    (p) => readZipEntry(bytes, pkgFiles ?? [], p, MAX_MANIFEST_BYTES2),
    extId
  );
  const status = gate.ok ? "pending_review" : "gate_failed";
  const note = gate.ok ? null : gate.problems.slice(0, 5).join("\uFF1B");
  const ins = await ctx.env.DB.prepare(
    `INSERT INTO submissions (user_id, ext_id, version, status, review_note, pkg_sha256, pkg_size, pkg_files)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)`
  ).bind(
    user.id,
    extId,
    version2,
    status,
    note,
    sha256,
    bytes.byteLength,
    pkgFiles ? JSON.stringify(pkgFiles) : null
  ).run();
  const subId = Number(ins.meta.last_row_id);
  await ctx.env.DB.prepare(
    `INSERT INTO submission_assets (submission_id, filename, size, sha256, storage_key)
     VALUES (?1, ?2, ?3, ?4, ?5)`
  ).bind(
    subId,
    pkg.name || `${extId}-${version2}.xhpack`,
    bytes.byteLength,
    sha256,
    `pending/${user.id}/${extId}/${version2}`
  ).run();
  const shots = form.getAll("screenshots[]").filter(
    (v) => v instanceof File
  );
  if (shots.length > 5) return fail(400, "bad_request", "\u622A\u56FE\u6700\u591A 5 \u5F20");
  for (const s of shots) {
    if (s.size > 2 * 1024 * 1024) return fail(400, "screenshot_too_large", "\u5355\u5F20\u622A\u56FE\u8D85\u8FC7 2MB");
  }
  const gateItems = gate.problems.map((p, i) => ({
    id: `g${i}`,
    label: p,
    ok: false,
    detail: ""
  }));
  return json({
    ok: true,
    id: subId,
    version: version2,
    status,
    review_note: note,
    // ↓ 通过时也要给 items（可以为空数组）——前端 v-for 依赖它存在
    gate: { passed: gate.ok, items: gateItems },
    screenshots: shots.length
  });
}
__name(submit, "submit");
async function mySubmissions(ctx) {
  const user = await requireDeveloper(ctx);
  if (user instanceof Response) return user;
  const url = ctx.url;
  const page = Math.max(1, Number(url.searchParams.get("page") ?? "1") || 1);
  const pageSize = Math.min(100, Math.max(1, Number(url.searchParams.get("page_size") ?? "20") || 20));
  const offset = (page - 1) * pageSize;
  const rows = await ctx.env.DB.prepare(
    `SELECT id, ext_id AS extId, version, status, review_note AS reviewNote,
            created_at AS createdAt, updated_at AS updatedAt
       FROM submissions WHERE user_id = ?1
      ORDER BY created_at DESC LIMIT ?2 OFFSET ?3`
  ).bind(user.id, pageSize, offset).all();
  const totalRow = await ctx.env.DB.prepare(
    `SELECT COUNT(*) AS total FROM submissions WHERE user_id = ?1`
  ).bind(user.id).first();
  const openCount = await ctx.env.DB.prepare(
    `SELECT COUNT(*) AS n FROM submissions
      WHERE user_id = ?1 AND status IN ('uploaded','pending_review','gate_failed')`
  ).bind(user.id).first();
  const total = totalRow?.total ?? 0;
  return json({
    items: rows.results ?? [],
    total,
    page,
    page_size: pageSize,
    // 约定 61：配额是「剩余」语义，且**只回剩余不回上限**（上限是运营参数，
    // 不进开源仓）。故客户端界面必须把话说成「还可 N 条」而不能自解释成「共 N 条」。
    quota: {
      drafts_remaining: Math.max(0, 5 - (openCount?.n ?? 0)),
      daily_submits_remaining: 3,
      published_remaining: Math.max(0, 10 - await countPublished(ctx, user.id))
    }
  });
}
__name(mySubmissions, "mySubmissions");
async function countPublished(ctx, userId) {
  const r2 = await ctx.env.DB.prepare(
    `SELECT COUNT(DISTINCT ext_id) AS n FROM submissions WHERE user_id = ?1 AND status = 'published'`
  ).bind(userId).first();
  return r2?.n ?? 0;
}
__name(countPublished, "countPublished");
async function submissionDetail(ctx, params) {
  const user = await requireDeveloper(ctx);
  if (user instanceof Response) return user;
  const id = Number(params.id);
  if (!Number.isInteger(id)) return fail(400, "bad_request", "id \u975E\u6CD5");
  const row = await ctx.env.DB.prepare(
    `SELECT id, ext_id AS extId, version, status, review_note AS reviewNote,
            created_at AS createdAt, updated_at AS updatedAt
       FROM submissions WHERE id = ?1 AND user_id = ?2`
  ).bind(id, user.id).first();
  if (!row) return fail(404, "not_found", "\u627E\u4E0D\u5230\u8BE5\u63D0\u4EA4\u8BB0\u5F55");
  return json(row);
}
__name(submissionDetail, "submissionDetail");
async function withdraw(ctx, params) {
  const user = await requireDeveloper(ctx);
  if (user instanceof Response) return user;
  const id = Number(params.id);
  if (!Number.isInteger(id)) return fail(400, "bad_request", "id \u975E\u6CD5");
  const cur = await ctx.env.DB.prepare(
    `SELECT status, version FROM submissions WHERE id = ?1 AND user_id = ?2`
  ).bind(id, user.id).first();
  if (!cur) return fail(404, "not_found", "\u627E\u4E0D\u5230\u8BE5\u63D0\u4EA4\u8BB0\u5F55");
  if (!OPEN_STATUSES.includes(cur.status)) {
    return fail(
      409,
      "not_withdrawable",
      `\u72B6\u6001 ${cur.status} \u4E0D\u53EF\u64A4\u56DE\uFF08\u53EF\u64A4\u56DE\uFF1A${OPEN_STATUSES.join(" / ")}\uFF09`
    );
  }
  await ctx.env.DB.prepare(
    `UPDATE submissions SET status = 'withdrawn', updated_at = ?2 WHERE id = ?1`
  ).bind(id, Date.now()).run();
  return json({ ok: true });
}
__name(withdraw, "withdraw");
function compareSemver(a, b) {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d < 0 ? -1 : 1;
  }
  return 0;
}
__name(compareSemver, "compareSemver");

// ../src/routes/admin.ts
async function submissionPackage(ctx, params) {
  const admin = await requireAdmin(ctx);
  if (admin instanceof Response) return admin;
  const id = Number(params.id);
  if (!Number.isInteger(id)) return fail(400, "bad_request", "id \u4E0D\u5408\u6CD5");
  const row = await ctx.env.DB.prepare(
    `SELECT pkg_sha256 AS pkgSha256, pkg_size AS pkgSize, status
       FROM submissions WHERE id = ?1`
  ).bind(id).first();
  if (!row) return fail(404, "not_found", "\u63D0\u4EA4\u4E0D\u5B58\u5728");
  if (row.status !== "approved") {
    return fail(409, "not_approved", `\u8BE5\u63D0\u4EA4\u662F ${row.status}\uFF0C\u53EA\u6709 approved \u80FD\u53D6\u5305\u4F53`);
  }
  if (!row.pkgSha256 || !row.pkgSize) return fail(409, "package_missing", "\u8BE5\u63D0\u4EA4\u6CA1\u6709\u5305\u4F53");
  const bytes = await getBlob(ctx.env.DB, row.pkgSha256, row.pkgSize);
  if (!bytes) return fail(409, "package_missing", "\u5305\u4F53\u5728\u5E93\u91CC\u627E\u4E0D\u5230");
  return new Response(bytes, {
    headers: {
      "content-type": "application/octet-stream",
      // ⚠️ 明确不缓存：包体按 sha256 变化，缓存错了就是「下到旧版本」。
      "cache-control": "no-store"
    }
  });
}
__name(submissionPackage, "submissionPackage");
var MAX_MANIFEST_BYTES3 = 256 * 1024;
function machineOk(env2, token) {
  const expect = env2.ADMIN_TOKEN;
  if (!expect || !token) return false;
  return timingSafeEqual(token, expect);
}
__name(machineOk, "machineOk");
async function requireAdmin(ctx) {
  if (machineOk(ctx.env, ctx.token)) {
    return {
      id: 0,
      github_id: null,
      username: "cli",
      email: null,
      avatar: null,
      role: "admin",
      is_developer: 1,
      developer_status: "approved",
      invite_redeemed: 1
    };
  }
  const user = await currentUser(ctx.env, ctx.token);
  if (!user) return unauthorized();
  if (user.role !== "admin") {
    return fail(403, "forbidden", "\u9700\u8981\u5E73\u53F0\u7BA1\u7406\u5458\u6743\u9650");
  }
  return user;
}
__name(requireAdmin, "requireAdmin");
async function listDevApplications(ctx) {
  const admin = await requireAdmin(ctx);
  if (admin instanceof Response) return admin;
  const rows = await ctx.env.DB.prepare(
    `SELECT d.id, d.status, d.reason, d.review_note AS reviewNote,
            d.created_at AS createdAt, u.username, u.developer_status AS developerStatus
       FROM dev_applications d JOIN users u ON u.id = d.user_id
      ORDER BY d.created_at ASC`
  ).all();
  return json({ items: rows.results ?? [] });
}
__name(listDevApplications, "listDevApplications");
async function approveDevApplication(ctx, params) {
  const admin = await requireAdmin(ctx);
  if (admin instanceof Response) return admin;
  const id = Number(params.id);
  if (!Number.isInteger(id)) return fail(400, "bad_request", "id \u4E0D\u5408\u6CD5");
  const body = await readJson(ctx.req);
  const note = typeof body.note === "string" ? body.note.slice(0, 500) : "";
  const app = await ctx.env.DB.prepare(
    `SELECT d.id, d.status, u.id AS userId
       FROM dev_applications d JOIN users u ON u.id = d.user_id
      WHERE d.id = ?1`
  ).bind(id).first();
  if (!app) return fail(404, "not_found", "\u7533\u8BF7\u4E0D\u5B58\u5728");
  if (app.status !== "pending") {
    return fail(409, "already_reviewed", `\u8BE5\u7533\u8BF7\u5DF2\u662F ${app.status} \u72B6\u6001`);
  }
  await ctx.env.DB.prepare(
    `UPDATE users SET developer_status = 'approved', is_developer = 1 WHERE id = ?1`
  ).bind(app.userId).run();
  await ctx.env.DB.prepare(
    `UPDATE dev_applications SET status = 'approved', review_note = ?1 WHERE id = ?2`
  ).bind(note, id).run();
  return json({ ok: true, id, status: "approved" });
}
__name(approveDevApplication, "approveDevApplication");
async function rejectDevApplication(ctx, params) {
  const admin = await requireAdmin(ctx);
  if (admin instanceof Response) return admin;
  const id = Number(params.id);
  if (!Number.isInteger(id)) return fail(400, "bad_request", "id \u4E0D\u5408\u6CD5");
  const body = await readJson(ctx.req);
  const note = typeof body.note === "string" ? body.note.trim() : "";
  if (!note) return fail(400, "bad_request", "\u9A73\u56DE\u5FC5\u987B\u586B\u5199\u7406\u7531\uFF08\u7533\u8BF7\u4EBA\u9700\u8981\u77E5\u9053\u6539\u4EC0\u4E48\uFF09");
  const r2 = await ctx.env.DB.prepare(
    `UPDATE dev_applications SET status = 'rejected', review_note = ?1 WHERE id = ?2 AND status = 'pending'`
  ).bind(note, id).run();
  if (!r2.meta.changes) return fail(409, "already_reviewed", "\u7533\u8BF7\u4E0D\u5B58\u5728\u6216\u5DF2\u5BA1\u6838");
  return json({ ok: true, id, status: "rejected" });
}
__name(rejectDevApplication, "rejectDevApplication");
async function listSubmissions(ctx) {
  const admin = await requireAdmin(ctx);
  if (admin instanceof Response) return admin;
  const rows = await ctx.env.DB.prepare(
    `SELECT s.id, s.ext_id AS extId, s.version, s.status, s.review_note AS reviewNote,
            s.pkg_sha256 AS pkgSha256, s.pkg_size AS pkgSize, s.created_at AS createdAt,
            u.username
       FROM submissions s JOIN users u ON u.id = s.user_id
      ORDER BY s.created_at ASC`
  ).all();
  const items = await Promise.all(
    (rows.results ?? []).map(async (r2) => ({
      ...r2,
      hasPackage: !!(r2.pkgSha256 && r2.pkgSize) && await getBlob(ctx.env.DB, String(r2.pkgSha256), Number(r2.pkgSize)) !== null
    }))
  );
  return json({ items });
}
__name(listSubmissions, "listSubmissions");
async function approveSubmission(ctx, params) {
  const admin = await requireAdmin(ctx);
  if (admin instanceof Response) return admin;
  const id = Number(params.id);
  if (!Number.isInteger(id)) return fail(400, "bad_request", "id \u4E0D\u5408\u6CD5");
  const row = await ctx.env.DB.prepare(
    `SELECT id, ext_id AS extId, version, status, pkg_sha256 AS pkgSha256, pkg_size AS pkgSize
       FROM submissions WHERE id = ?1`
  ).bind(id).first();
  if (!row) return fail(404, "not_found", "\u63D0\u4EA4\u4E0D\u5B58\u5728");
  if (row.status !== "pending_review") {
    return fail(409, "already_reviewed", `\u8BE5\u63D0\u4EA4\u5DF2\u662F ${row.status} \u72B6\u6001`);
  }
  if (!row.pkgSha256 || !row.pkgSize) {
    return fail(409, "package_missing", "\u8BE5\u63D0\u4EA4\u6CA1\u6709\u5305\u4F53\uFF08\u65E7\u7248\u672C\u63D0\u4EA4\uFF0C\u90A3\u65F6\u8FD8\u6CA1\u5B58\u5305\uFF09");
  }
  const bytes = await getBlob(ctx.env.DB, row.pkgSha256, row.pkgSize);
  if (!bytes) return fail(409, "package_missing", "\u5305\u4F53\u5728\u5E93\u91CC\u627E\u4E0D\u5230\uFF0C\u65E0\u6CD5\u5BA1\u6838");
  const entries = listZipEntries(bytes);
  const gate = await runGate(
    entries,
    (p) => readZipEntry(bytes, entries, p, MAX_MANIFEST_BYTES3),
    row.extId
  );
  if (!gate.ok) {
    await ctx.env.DB.prepare(
      `UPDATE submissions SET status = 'gate_failed', review_note = ?1 WHERE id = ?2`
    ).bind(gate.problems.slice(0, 5).join("\uFF1B"), id).run();
    return fail(422, "gate_failed", gate.problems[0] ?? "\u672A\u901A\u8FC7\u5173\u5361", { problems: gate.problems });
  }
  const body = await readJson(ctx.req);
  const note = typeof body.note === "string" ? body.note.slice(0, 500) : "";
  await ctx.env.DB.prepare(
    `UPDATE submissions SET status = 'approved', review_note = ?1, updated_at = ?2 WHERE id = ?3`
  ).bind(note, Date.now(), id).run();
  return json({
    ok: true,
    id,
    status: "approved",
    // 明确告诉审核人**下一步**：状态改了不等于已上架
    next: "\u5DF2\u901A\u8FC7\u5BA1\u6838\u3002\u4ECD\u9700\u8FD0\u884C `npm run publish:approved` \u624D\u4F1A\u8FDB\u5E02\u573A\u6E05\u5355\u5E76\u90E8\u7F72\u3002"
  });
}
__name(approveSubmission, "approveSubmission");
async function rejectSubmission(ctx, params) {
  const admin = await requireAdmin(ctx);
  if (admin instanceof Response) return admin;
  const id = Number(params.id);
  if (!Number.isInteger(id)) return fail(400, "bad_request", "id \u4E0D\u5408\u6CD5");
  const body = await readJson(ctx.req);
  const note = typeof body.note === "string" ? body.note.trim() : "";
  if (!note) return fail(400, "bad_request", "\u9A73\u56DE\u5FC5\u987B\u586B\u5199\u7406\u7531\uFF08\u4F5C\u8005\u9700\u8981\u77E5\u9053\u6539\u4EC0\u4E48\uFF09");
  const r2 = await ctx.env.DB.prepare(
    `UPDATE submissions SET status = 'rejected', review_note = ?1, updated_at = ?2
      WHERE id = ?3 AND status IN ('pending_review','gate_failed')`
  ).bind(note, Date.now(), id).run();
  if (!r2.meta.changes) return fail(409, "already_reviewed", "\u63D0\u4EA4\u4E0D\u5B58\u5728\u6216\u5DF2\u5BA1\u6838");
  return json({ ok: true, id, status: "rejected" });
}
__name(rejectSubmission, "rejectSubmission");
async function deleteSubmission(ctx, params) {
  const admin = await requireAdmin(ctx);
  if (admin instanceof Response) return admin;
  const id = Number(params.id);
  if (!Number.isInteger(id)) return fail(400, "bad_request", "id \u4E0D\u5408\u6CD5");
  const row = await ctx.env.DB.prepare(
    `SELECT status, pkg_sha256 AS pkgSha256 FROM submissions WHERE id = ?1`
  ).bind(id).first();
  if (!row) return fail(404, "not_found", "\u63D0\u4EA4\u4E0D\u5B58\u5728");
  if (row.status === "published") {
    return fail(
      409,
      "still_published",
      "\u8FD9\u6761\u5DF2\u4E0A\u67B6\uFF0C\u5220\u4E0D\u6389\u3002\u5148\u5728\u5E02\u573A\u628A\u5B83\u4E0B\u67B6\uFF08\u4F1A\u5199 revoked \u5E76\u91CD\u7B7E\u6E05\u5355\uFF09\uFF0C\u518D\u5220\u8FD9\u6761\u8BB0\u5F55\u3002"
    );
  }
  if (!DELETABLE_STATUSES.includes(row.status)) {
    return fail(
      409,
      "not_deletable",
      `\u53EA\u80FD\u5220\u5DF2\u8D70\u5B8C\u6D41\u7A0B\u7684\u63D0\u4EA4\uFF08${DELETABLE_STATUSES.join(" / ")}\uFF09\uFF0C\u8FD9\u6761\u662F ${row.status}\u3002\u8D70\u5B8C\u6D41\u7A0B\u524D\u8BF7\u8D70\u64A4\u56DE\u3002`
    );
  }
  const placeholders = DELETABLE_STATUSES.map((_, i) => `?${i + 2}`).join(", ");
  const del = await ctx.env.DB.prepare(
    `DELETE FROM submissions WHERE id = ?1 AND status IN (${placeholders})`
  ).bind(id, ...DELETABLE_STATUSES).run();
  if (!del.meta.changes) {
    return fail(409, "not_deletable", "\u63D0\u4EA4\u4E0D\u5B58\u5728\uFF0C\u6216\u5B83\u7684\u72B6\u6001\u5DF2\u53D8\uFF08\u8BF7\u5237\u65B0\u540E\u91CD\u8BD5\uFF09");
  }
  let blobFreed = false;
  if (row.pkgSha256) {
    const stillUsed = await ctx.env.DB.prepare(
      `SELECT COUNT(*) AS n FROM submissions WHERE pkg_sha256 = ?1`
    ).bind(row.pkgSha256).first();
    if ((stillUsed?.n ?? 0) === 0) {
      await ctx.env.DB.prepare(`DELETE FROM pkg_blobs WHERE sha256 = ?1`).bind(row.pkgSha256).run();
      blobFreed = true;
    }
  }
  return json({ ok: true, id, deleted: row.status, blobFreed });
}
__name(deleteSubmission, "deleteSubmission");
async function adminHealth(ctx) {
  const admin = await requireAdmin(ctx);
  if (admin instanceof Response) return admin;
  return json({ ok: true, marker: "admin-delete-v1" });
}
__name(adminHealth, "adminHealth");
var DELETABLE_STATUSES = ["approved", "rejected", "withdrawn"];
async function markSubmissionPublished(ctx, params) {
  const admin = await requireAdmin(ctx);
  if (admin instanceof Response) return admin;
  const id = Number(params.id);
  if (!Number.isInteger(id)) return fail(400, "bad_request", "id \u4E0D\u5408\u6CD5");
  const r2 = await ctx.env.DB.prepare(
    `UPDATE submissions SET status = 'published', updated_at = ?1
      WHERE id = ?2 AND status = 'approved'`
  ).bind(Date.now(), id).run();
  if (!r2.meta.changes) {
    const row = await ctx.env.DB.prepare(`SELECT status FROM submissions WHERE id = ?1`).bind(id).first();
    if (!row) return fail(404, "not_found", "\u63D0\u4EA4\u4E0D\u5B58\u5728");
    if (row.status === "published") return json({ ok: true, id, status: "published" });
    return fail(409, "not_approved", `\u53EA\u6709 approved \u80FD\u6807\u8BB0\u4E3A\u5DF2\u4E0A\u67B6\uFF0C\u8FD9\u6761\u662F ${row.status}`);
  }
  return json({ ok: true, id, status: "published" });
}
__name(markSubmissionPublished, "markSubmissionPublished");

// ../src/handle.ts
var r = new Router();
r.add("POST", "/api/v1/auth/github/device/start", (c) => deviceStart(c));
r.add("POST", "/api/v1/auth/github/device/poll", (c) => devicePoll(c));
r.add("POST", "/api/v1/auth/email/send", (c) => send2(c));
r.add("POST", "/api/v1/auth/email/verify", (c) => verify(c));
r.add("GET", "/me", (c) => me(c));
r.add("POST", "/api/v1/me/redeem", (c) => redeem(c));
r.add("GET", "/api/v1/me/device-tokens", (c) => deviceTokens(c));
r.add("POST", "/api/v1/me/device-tokens/:id/revoke", (c) => deviceRevoke(c, c.params));
r.add("GET", "/api/v1/ai/models", (c) => models(c));
r.add("POST", "/api/v1/dev/apply", (c) => apply(c));
r.add("GET", "/api/v1/dev/apply", (c) => applyStatus(c));
r.add("POST", "/api/v1/dev/submissions", (c) => submit(c));
r.add("GET", "/api/v1/dev/submissions", (c) => mySubmissions(c));
r.add("GET", "/api/v1/dev/submissions/:id", (c) => submissionDetail(c, c.params));
r.add("POST", "/api/v1/dev/submissions/:id/withdraw", (c) => withdraw(c, c.params));
r.add("POST", OPENAI_COMPAT.post_chat_completions, (c) => chatCompletions(c));
r.add("GET", "/api/v1/admin/dev-applications", (c) => listDevApplications(c));
r.add("POST", "/api/v1/admin/dev-applications/:id/approve", (c) => approveDevApplication(c, c.params));
r.add("POST", "/api/v1/admin/dev-applications/:id/reject", (c) => rejectDevApplication(c, c.params));
r.add("GET", "/api/v1/admin/health", (c) => adminHealth(c));
r.add("GET", "/api/v1/admin/submissions", (c) => listSubmissions(c));
r.add("POST", "/api/v1/admin/submissions/:id/approve", (c) => approveSubmission(c, c.params));
r.add("POST", "/api/v1/admin/submissions/:id/reject", (c) => rejectSubmission(c, c.params));
r.add("DELETE", "/api/v1/admin/submissions/:id", (c) => deleteSubmission(c, c.params));
r.add(
  "POST",
  "/api/v1/admin/submissions/:id/mark-published",
  (c) => markSubmissionPublished(c, c.params)
);
r.add("GET", "/api/v1/admin/submissions/:id/package", (c) => submissionPackage(c, c.params));
var STATIC_EXACT = /* @__PURE__ */ new Set(["/api/v1/market/registry", "/api/v1/app/update"]);
function is_static_asset(pathname) {
  if (STATIC_EXACT.has(pathname)) return true;
  if (STATIC_EXACT.has(pathname.replace(/\.sig$/, ""))) return true;
  return pathname.startsWith("/packages/") || pathname.startsWith("/downloads/");
}
__name(is_static_asset, "is_static_asset");
async function handleRequest(req, env2) {
  const url = new URL(req.url);
  const method = req.method.toUpperCase();
  if (is_static_asset(url.pathname) && "ASSETS" in env2 && env2.ASSETS) {
    const res = await env2.ASSETS.fetch(req);
    if (res.status !== 404) return res;
    return fail(404, "static_asset_missing", `\u9759\u6001\u8D44\u6E90\u672A\u90E8\u7F72\uFF1A${url.pathname}\uFF08\u6E05\u5355\u4E0E\u5305\u7531 public/ \u51B3\u5B9A\uFF0C\u8DD1\u4E00\u6B21 seed-manifests \u518D\u90E8\u7F72\uFF09`);
  }
  const ctx = {
    req,
    env: env2,
    url,
    params: {},
    token: req.headers.get("authorization")?.replace(/^Bearer\s+/i, "").trim() || void 0
  };
  const hit = r.resolve(method, url.pathname);
  if (hit.kind === "not_found") {
    return fail(404, "not_found", `\u6CA1\u6709\u8FD9\u4E2A\u63A5\u53E3\uFF1A${method} ${url.pathname}`);
  }
  if (hit.kind === "method_not_allowed") {
    return fail(
      405,
      "method_not_allowed",
      `${url.pathname} \u4E0D\u63A5\u53D7 ${method}\uFF08\u63A5\u53D7\uFF1A${hit.allow.join(", ")}\uFF09`,
      {},
      { allow: hit.allow.join(", ") }
    );
  }
  ctx.params = hit.params;
  try {
    return await hit.handler(ctx);
  } catch (e) {
    console.error(`[m-hub] ${method} ${url.pathname} \u672A\u5904\u7406\u5F02\u5E38:`, e);
    return fail(500, "server_error", "\u670D\u52A1\u7AEF\u5185\u90E8\u9519\u8BEF");
  }
}
__name(handleRequest, "handleRequest");

// v1/chat/completions.ts
var onRequest = /* @__PURE__ */ __name((ctx) => handleRequest(ctx.request, ctx.env), "onRequest");

// api/v1/[[path]].ts
var onRequest2 = /* @__PURE__ */ __name((ctx) => handleRequest(ctx.request, ctx.env), "onRequest");

// me.ts
var onRequest3 = /* @__PURE__ */ __name((ctx) => handleRequest(ctx.request, ctx.env), "onRequest");

// ../.wrangler/tmp/pages-UwqWYr/functionsRoutes-0.5486897307132126.mjs
var routes = [
  {
    routePath: "/v1/chat/completions",
    mountPath: "/v1/chat",
    method: "",
    middlewares: [],
    modules: [onRequest]
  },
  {
    routePath: "/api/v1/:path*",
    mountPath: "/api/v1",
    method: "",
    middlewares: [],
    modules: [onRequest2]
  },
  {
    routePath: "/me",
    mountPath: "/",
    method: "",
    middlewares: [],
    modules: [onRequest3]
  }
];

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/path-to-regexp/dist.es2015/index.js
function lexer(str) {
  var tokens = [];
  var i = 0;
  while (i < str.length) {
    var char = str[i];
    if (char === "*" || char === "+" || char === "?") {
      tokens.push({ type: "MODIFIER", index: i, value: str[i++] });
      continue;
    }
    if (char === "\\") {
      tokens.push({ type: "ESCAPED_CHAR", index: i++, value: str[i++] });
      continue;
    }
    if (char === "{") {
      tokens.push({ type: "OPEN", index: i, value: str[i++] });
      continue;
    }
    if (char === "}") {
      tokens.push({ type: "CLOSE", index: i, value: str[i++] });
      continue;
    }
    if (char === ":") {
      var name = "";
      var j = i + 1;
      while (j < str.length) {
        var code = str.charCodeAt(j);
        if (
          // `0-9`
          code >= 48 && code <= 57 || // `A-Z`
          code >= 65 && code <= 90 || // `a-z`
          code >= 97 && code <= 122 || // `_`
          code === 95
        ) {
          name += str[j++];
          continue;
        }
        break;
      }
      if (!name)
        throw new TypeError("Missing parameter name at ".concat(i));
      tokens.push({ type: "NAME", index: i, value: name });
      i = j;
      continue;
    }
    if (char === "(") {
      var count3 = 1;
      var pattern = "";
      var j = i + 1;
      if (str[j] === "?") {
        throw new TypeError('Pattern cannot start with "?" at '.concat(j));
      }
      while (j < str.length) {
        if (str[j] === "\\") {
          pattern += str[j++] + str[j++];
          continue;
        }
        if (str[j] === ")") {
          count3--;
          if (count3 === 0) {
            j++;
            break;
          }
        } else if (str[j] === "(") {
          count3++;
          if (str[j + 1] !== "?") {
            throw new TypeError("Capturing groups are not allowed at ".concat(j));
          }
        }
        pattern += str[j++];
      }
      if (count3)
        throw new TypeError("Unbalanced pattern at ".concat(i));
      if (!pattern)
        throw new TypeError("Missing pattern at ".concat(i));
      tokens.push({ type: "PATTERN", index: i, value: pattern });
      i = j;
      continue;
    }
    tokens.push({ type: "CHAR", index: i, value: str[i++] });
  }
  tokens.push({ type: "END", index: i, value: "" });
  return tokens;
}
__name(lexer, "lexer");
function parse(str, options) {
  if (options === void 0) {
    options = {};
  }
  var tokens = lexer(str);
  var _a = options.prefixes, prefixes = _a === void 0 ? "./" : _a, _b = options.delimiter, delimiter = _b === void 0 ? "/#?" : _b;
  var result = [];
  var key = 0;
  var i = 0;
  var path = "";
  var tryConsume = /* @__PURE__ */ __name(function(type) {
    if (i < tokens.length && tokens[i].type === type)
      return tokens[i++].value;
  }, "tryConsume");
  var mustConsume = /* @__PURE__ */ __name(function(type) {
    var value2 = tryConsume(type);
    if (value2 !== void 0)
      return value2;
    var _a2 = tokens[i], nextType = _a2.type, index = _a2.index;
    throw new TypeError("Unexpected ".concat(nextType, " at ").concat(index, ", expected ").concat(type));
  }, "mustConsume");
  var consumeText = /* @__PURE__ */ __name(function() {
    var result2 = "";
    var value2;
    while (value2 = tryConsume("CHAR") || tryConsume("ESCAPED_CHAR")) {
      result2 += value2;
    }
    return result2;
  }, "consumeText");
  var isSafe = /* @__PURE__ */ __name(function(value2) {
    for (var _i = 0, delimiter_1 = delimiter; _i < delimiter_1.length; _i++) {
      var char2 = delimiter_1[_i];
      if (value2.indexOf(char2) > -1)
        return true;
    }
    return false;
  }, "isSafe");
  var safePattern = /* @__PURE__ */ __name(function(prefix2) {
    var prev = result[result.length - 1];
    var prevText = prefix2 || (prev && typeof prev === "string" ? prev : "");
    if (prev && !prevText) {
      throw new TypeError('Must have text between two parameters, missing text after "'.concat(prev.name, '"'));
    }
    if (!prevText || isSafe(prevText))
      return "[^".concat(escapeString(delimiter), "]+?");
    return "(?:(?!".concat(escapeString(prevText), ")[^").concat(escapeString(delimiter), "])+?");
  }, "safePattern");
  while (i < tokens.length) {
    var char = tryConsume("CHAR");
    var name = tryConsume("NAME");
    var pattern = tryConsume("PATTERN");
    if (name || pattern) {
      var prefix = char || "";
      if (prefixes.indexOf(prefix) === -1) {
        path += prefix;
        prefix = "";
      }
      if (path) {
        result.push(path);
        path = "";
      }
      result.push({
        name: name || key++,
        prefix,
        suffix: "",
        pattern: pattern || safePattern(prefix),
        modifier: tryConsume("MODIFIER") || ""
      });
      continue;
    }
    var value = char || tryConsume("ESCAPED_CHAR");
    if (value) {
      path += value;
      continue;
    }
    if (path) {
      result.push(path);
      path = "";
    }
    var open = tryConsume("OPEN");
    if (open) {
      var prefix = consumeText();
      var name_1 = tryConsume("NAME") || "";
      var pattern_1 = tryConsume("PATTERN") || "";
      var suffix = consumeText();
      mustConsume("CLOSE");
      result.push({
        name: name_1 || (pattern_1 ? key++ : ""),
        pattern: name_1 && !pattern_1 ? safePattern(prefix) : pattern_1,
        prefix,
        suffix,
        modifier: tryConsume("MODIFIER") || ""
      });
      continue;
    }
    mustConsume("END");
  }
  return result;
}
__name(parse, "parse");
function match(str, options) {
  var keys = [];
  var re = pathToRegexp(str, keys, options);
  return regexpToFunction(re, keys, options);
}
__name(match, "match");
function regexpToFunction(re, keys, options) {
  if (options === void 0) {
    options = {};
  }
  var _a = options.decode, decode = _a === void 0 ? function(x) {
    return x;
  } : _a;
  return function(pathname) {
    var m = re.exec(pathname);
    if (!m)
      return false;
    var path = m[0], index = m.index;
    var params = /* @__PURE__ */ Object.create(null);
    var _loop_1 = /* @__PURE__ */ __name(function(i2) {
      if (m[i2] === void 0)
        return "continue";
      var key = keys[i2 - 1];
      if (key.modifier === "*" || key.modifier === "+") {
        params[key.name] = m[i2].split(key.prefix + key.suffix).map(function(value) {
          return decode(value, key);
        });
      } else {
        params[key.name] = decode(m[i2], key);
      }
    }, "_loop_1");
    for (var i = 1; i < m.length; i++) {
      _loop_1(i);
    }
    return { path, index, params };
  };
}
__name(regexpToFunction, "regexpToFunction");
function escapeString(str) {
  return str.replace(/([.+*?=^!:${}()[\]|/\\])/g, "\\$1");
}
__name(escapeString, "escapeString");
function flags(options) {
  return options && options.sensitive ? "" : "i";
}
__name(flags, "flags");
function regexpToRegexp(path, keys) {
  if (!keys)
    return path;
  var groupsRegex = /\((?:\?<(.*?)>)?(?!\?)/g;
  var index = 0;
  var execResult = groupsRegex.exec(path.source);
  while (execResult) {
    keys.push({
      // Use parenthesized substring match if available, index otherwise
      name: execResult[1] || index++,
      prefix: "",
      suffix: "",
      modifier: "",
      pattern: ""
    });
    execResult = groupsRegex.exec(path.source);
  }
  return path;
}
__name(regexpToRegexp, "regexpToRegexp");
function arrayToRegexp(paths, keys, options) {
  var parts = paths.map(function(path) {
    return pathToRegexp(path, keys, options).source;
  });
  return new RegExp("(?:".concat(parts.join("|"), ")"), flags(options));
}
__name(arrayToRegexp, "arrayToRegexp");
function stringToRegexp(path, keys, options) {
  return tokensToRegexp(parse(path, options), keys, options);
}
__name(stringToRegexp, "stringToRegexp");
function tokensToRegexp(tokens, keys, options) {
  if (options === void 0) {
    options = {};
  }
  var _a = options.strict, strict = _a === void 0 ? false : _a, _b = options.start, start = _b === void 0 ? true : _b, _c = options.end, end = _c === void 0 ? true : _c, _d = options.encode, encode = _d === void 0 ? function(x) {
    return x;
  } : _d, _e = options.delimiter, delimiter = _e === void 0 ? "/#?" : _e, _f = options.endsWith, endsWith = _f === void 0 ? "" : _f;
  var endsWithRe = "[".concat(escapeString(endsWith), "]|$");
  var delimiterRe = "[".concat(escapeString(delimiter), "]");
  var route = start ? "^" : "";
  for (var _i = 0, tokens_1 = tokens; _i < tokens_1.length; _i++) {
    var token = tokens_1[_i];
    if (typeof token === "string") {
      route += escapeString(encode(token));
    } else {
      var prefix = escapeString(encode(token.prefix));
      var suffix = escapeString(encode(token.suffix));
      if (token.pattern) {
        if (keys)
          keys.push(token);
        if (prefix || suffix) {
          if (token.modifier === "+" || token.modifier === "*") {
            var mod = token.modifier === "*" ? "?" : "";
            route += "(?:".concat(prefix, "((?:").concat(token.pattern, ")(?:").concat(suffix).concat(prefix, "(?:").concat(token.pattern, "))*)").concat(suffix, ")").concat(mod);
          } else {
            route += "(?:".concat(prefix, "(").concat(token.pattern, ")").concat(suffix, ")").concat(token.modifier);
          }
        } else {
          if (token.modifier === "+" || token.modifier === "*") {
            throw new TypeError('Can not repeat "'.concat(token.name, '" without a prefix and suffix'));
          }
          route += "(".concat(token.pattern, ")").concat(token.modifier);
        }
      } else {
        route += "(?:".concat(prefix).concat(suffix, ")").concat(token.modifier);
      }
    }
  }
  if (end) {
    if (!strict)
      route += "".concat(delimiterRe, "?");
    route += !options.endsWith ? "$" : "(?=".concat(endsWithRe, ")");
  } else {
    var endToken = tokens[tokens.length - 1];
    var isEndDelimited = typeof endToken === "string" ? delimiterRe.indexOf(endToken[endToken.length - 1]) > -1 : endToken === void 0;
    if (!strict) {
      route += "(?:".concat(delimiterRe, "(?=").concat(endsWithRe, "))?");
    }
    if (!isEndDelimited) {
      route += "(?=".concat(delimiterRe, "|").concat(endsWithRe, ")");
    }
  }
  return new RegExp(route, flags(options));
}
__name(tokensToRegexp, "tokensToRegexp");
function pathToRegexp(path, keys, options) {
  if (path instanceof RegExp)
    return regexpToRegexp(path, keys);
  if (Array.isArray(path))
    return arrayToRegexp(path, keys, options);
  return stringToRegexp(path, keys, options);
}
__name(pathToRegexp, "pathToRegexp");

// ../../../../../../.npm/_npx/d77349f55c2be1c0/node_modules/wrangler/templates/pages-template-worker.ts
var escapeRegex = /[.+?^${}()|[\]\\]/g;
function* executeRequest(request) {
  const requestPath = new URL(request.url).pathname;
  for (const route of [...routes].reverse()) {
    if (route.method && route.method !== request.method) {
      continue;
    }
    const routeMatcher = match(route.routePath.replace(escapeRegex, "\\$&"), {
      end: false
    });
    const mountMatcher = match(route.mountPath.replace(escapeRegex, "\\$&"), {
      end: false
    });
    const matchResult = routeMatcher(requestPath);
    const mountMatchResult = mountMatcher(requestPath);
    if (matchResult && mountMatchResult) {
      for (const handler of route.middlewares.flat()) {
        yield {
          handler,
          params: matchResult.params,
          path: mountMatchResult.path
        };
      }
    }
  }
  for (const route of routes) {
    if (route.method && route.method !== request.method) {
      continue;
    }
    const routeMatcher = match(route.routePath.replace(escapeRegex, "\\$&"), {
      end: true
    });
    const mountMatcher = match(route.mountPath.replace(escapeRegex, "\\$&"), {
      end: false
    });
    const matchResult = routeMatcher(requestPath);
    const mountMatchResult = mountMatcher(requestPath);
    if (matchResult && mountMatchResult && route.modules.length) {
      for (const handler of route.modules.flat()) {
        yield {
          handler,
          params: matchResult.params,
          path: matchResult.path
        };
      }
      break;
    }
  }
}
__name(executeRequest, "executeRequest");
var pages_template_worker_default = {
  async fetch(originalRequest, env2, workerContext) {
    let request = originalRequest;
    const handlerIterator = executeRequest(request);
    let data = {};
    let isFailOpen = false;
    const next = /* @__PURE__ */ __name(async (input, init) => {
      if (input !== void 0) {
        let url = input;
        if (typeof input === "string") {
          url = new URL(input, request.url).toString();
        }
        request = new Request(url, init);
      }
      const result = handlerIterator.next();
      if (result.done === false) {
        const { handler, params, path } = result.value;
        const context2 = {
          request: new Request(request.clone()),
          functionPath: path,
          next,
          params,
          get data() {
            return data;
          },
          set data(value) {
            if (typeof value !== "object" || value === null) {
              throw new Error("context.data must be an object");
            }
            data = value;
          },
          env: env2,
          waitUntil: workerContext.waitUntil.bind(workerContext),
          passThroughOnException: /* @__PURE__ */ __name(() => {
            isFailOpen = true;
          }, "passThroughOnException")
        };
        const response = await handler(context2);
        if (!(response instanceof Response)) {
          throw new Error("Your Pages function should return a Response");
        }
        return cloneResponse(response);
      } else if ("ASSETS") {
        const response = await env2["ASSETS"].fetch(request);
        return cloneResponse(response);
      } else {
        const response = await fetch(request);
        return cloneResponse(response);
      }
    }, "next");
    try {
      return await next();
    } catch (error3) {
      if (isFailOpen) {
        const response = await env2["ASSETS"].fetch(request);
        return cloneResponse(response);
      }
      throw error3;
    }
  }
};
var cloneResponse = /* @__PURE__ */ __name((response) => (
  // https://fetch.spec.whatwg.org/#null-body-status
  new Response(
    [101, 204, 205, 304].includes(response.status) ? null : response.body,
    response
  )
), "cloneResponse");
export {
  pages_template_worker_default as default
};
