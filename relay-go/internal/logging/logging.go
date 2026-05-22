// Package logging wires log/slog with a terminal handler at a configurable
// level and a file handler at debug, mirroring the Rust relay's two-sink
// layout.
package logging

import (
	"context"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
)

// Level mirrors the Rust CLI's --log-level enum. "trace" folds into slog debug.
type Level string

const (
	LevelOff   Level = "off"
	LevelError Level = "error"
	LevelWarn  Level = "warn"
	LevelInfo  Level = "info"
	LevelDebug Level = "debug"
	LevelTrace Level = "trace"
)

// ParseLevel normalises user input. Unknown values fall back to "off".
func ParseLevel(s string) Level {
	switch strings.ToLower(s) {
	case "off", "error", "warn", "info", "debug", "trace":
		return Level(strings.ToLower(s))
	default:
		return LevelOff
	}
}

func (l Level) slog() slog.Level {
	switch l {
	case LevelError:
		return slog.LevelError
	case LevelWarn:
		return slog.LevelWarn
	case LevelInfo:
		return slog.LevelInfo
	case LevelDebug, LevelTrace:
		return slog.LevelDebug
	default: // off
		return slog.LevelError + 1000 // effectively disables output
	}
}

// Setup installs a slog default logger that writes to:
//   - stderr at the chosen terminal level (or never, if "off")
//   - the file at logPath at debug level (always)
//
// If the file cannot be opened, the function logs a warning to stderr and
// continues with only the terminal handler. The returned closer flushes the
// file handler on shutdown.
func Setup(termLevel Level, logPath string) io.Closer {
	termHandler := slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{
		Level: termLevel.slog(),
	})

	if err := os.MkdirAll(filepath.Dir(logPath), 0755); err != nil {
		slog.SetDefault(slog.New(termHandler))
		slog.Warn("could not create log dir", "path", logPath, "err", err)
		return io.NopCloser(nil)
	}
	f, err := os.OpenFile(logPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		slog.SetDefault(slog.New(termHandler))
		slog.Warn("could not open log file; terminal-only logging", "path", logPath, "err", err)
		return io.NopCloser(nil)
	}
	fileHandler := slog.NewJSONHandler(f, &slog.HandlerOptions{
		Level:     slog.LevelDebug,
		AddSource: true,
	})

	slog.SetDefault(slog.New(multiHandler{handlers: []slog.Handler{termHandler, fileHandler}}))
	return f
}

// multiHandler fans slog records out to multiple handlers.
type multiHandler struct{ handlers []slog.Handler }

func (m multiHandler) Enabled(ctx context.Context, l slog.Level) bool {
	for _, h := range m.handlers {
		if h.Enabled(ctx, l) {
			return true
		}
	}
	return false
}

func (m multiHandler) Handle(ctx context.Context, r slog.Record) error {
	for _, h := range m.handlers {
		if h.Enabled(ctx, r.Level) {
			if err := h.Handle(ctx, r.Clone()); err != nil {
				return err
			}
		}
	}
	return nil
}

func (m multiHandler) WithAttrs(as []slog.Attr) slog.Handler {
	out := make([]slog.Handler, len(m.handlers))
	for i, h := range m.handlers {
		out[i] = h.WithAttrs(as)
	}
	return multiHandler{handlers: out}
}

func (m multiHandler) WithGroup(name string) slog.Handler {
	out := make([]slog.Handler, len(m.handlers))
	for i, h := range m.handlers {
		out[i] = h.WithGroup(name)
	}
	return multiHandler{handlers: out}
}
