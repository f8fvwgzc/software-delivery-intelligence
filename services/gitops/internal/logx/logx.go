// Package logx sets up structured logging: JSON lines when LOG_FORMAT=json
// (containers), human-readable text otherwise, as every SDI service does.
//
// The engine and client-go log through logr and klog. Their informational
// chatter (every apply, every watch restart, every task list) is routed one
// level below ours, to debug, so the default level shows only what the
// service itself says, plus their errors as warnings. Nothing in either is
// allowed to carry a kubeconfig: the engine never logs rest.Config, and the
// manifests it can log at its verbose levels never reach our handler, whose
// lowest level is debug.
package logx

import (
	"context"
	"io"
	"log/slog"
	"os"
	"strings"

	"github.com/go-logr/logr"
	"k8s.io/klog/v2"
)

// Setup installs the process-wide logger and returns it.
func Setup(service string) *slog.Logger {
	return setup(os.Stderr, os.Getenv("LOG_FORMAT"), os.Getenv("LOG_LEVEL"), service)
}

func setup(w io.Writer, format, level, service string) *slog.Logger {
	opts := &slog.HandlerOptions{Level: parseLevel(level)}
	var handler slog.Handler
	if strings.EqualFold(strings.TrimSpace(format), "json") {
		handler = slog.NewJSONHandler(w, opts)
	} else {
		handler = slog.NewTextHandler(w, opts)
	}
	logger := slog.New(handler).With("service", service)
	slog.SetDefault(logger)
	// client-go and the kubectl libraries log through klog.
	klog.SetLogger(Engine(logger))
	return logger
}

func parseLevel(raw string) slog.Level {
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case "debug":
		return slog.LevelDebug
	case "warn", "warning":
		return slog.LevelWarn
	case "error":
		return slog.LevelError
	}
	return slog.LevelInfo
}

// Engine is a logr.Logger for the gitops engine and client-go: their Info
// becomes our debug (V(n) goes further down, below anything we print) and
// their Error becomes a warning.
func Engine(logger *slog.Logger) logr.Logger {
	return logr.New(&engineSink{logger: logger.With("component", "engine")})
}

type engineSink struct {
	logger *slog.Logger
	name   string
}

func (s *engineSink) Init(logr.RuntimeInfo) {}

func (s *engineSink) Enabled(level int) bool {
	return s.logger.Enabled(context.Background(), slog.LevelDebug-slog.Level(level))
}

func (s *engineSink) Info(level int, msg string, kv ...any) {
	s.logger.Log(context.Background(), slog.LevelDebug-slog.Level(level), s.prefix(msg), sanitize(kv)...)
}

func (s *engineSink) Error(err error, msg string, kv ...any) {
	args := append([]any{"error", errText(err)}, sanitize(kv)...)
	s.logger.Log(context.Background(), slog.LevelWarn, s.prefix(msg), args...)
}

func (s *engineSink) WithValues(kv ...any) logr.LogSink {
	return &engineSink{logger: s.logger.With(sanitize(kv)...), name: s.name}
}

func (s *engineSink) WithName(name string) logr.LogSink {
	if s.name != "" {
		name = s.name + "." + name
	}
	return &engineSink{logger: s.logger, name: name}
}

func (s *engineSink) prefix(msg string) string {
	if s.name == "" {
		return msg
	}
	return s.name + ": " + msg
}

func errText(err error) string {
	if err == nil {
		return ""
	}
	return err.Error()
}

// sensitiveKeys are never logged whatever logs them.
var sensitiveKeys = map[string]bool{
	"token": true, "password": true, "kubeconfig": true, "config": true,
	"bearertoken": true, "secret": true, "data": true, "stringdata": true,
}

func sanitize(kv []any) []any {
	out := make([]any, 0, len(kv))
	for i := 0; i+1 < len(kv); i += 2 {
		key, _ := kv[i].(string)
		if sensitiveKeys[strings.ToLower(key)] {
			out = append(out, key, "[redacted]")
			continue
		}
		out = append(out, kv[i], kv[i+1])
	}
	return out
}
