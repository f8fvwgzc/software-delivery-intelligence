package logx

import (
	"bytes"
	"errors"
	"strings"
	"testing"
)

func TestEngineChatterIsDebugAndSensitiveValuesAreRedacted(t *testing.T) {
	var buf bytes.Buffer
	logger := setup(&buf, "json", "info", "test")
	engine := Engine(logger)
	engine.Info("applying resource", "kind", "Secret")
	engine.V(1).Info("manifest", "data", "c2VjcmV0")
	if buf.Len() != 0 {
		t.Fatalf("engine info must not reach the info level: %s", buf.String())
	}
	engine.Error(errors.New("boom"), "apply failed", "token", "ghp_secret", "name", "web")
	out := buf.String()
	if !strings.Contains(out, `"level":"WARN"`) || !strings.Contains(out, "apply failed") || !strings.Contains(out, `"name":"web"`) {
		t.Fatalf("engine errors are warnings: %s", out)
	}
	if strings.Contains(out, "ghp_secret") || !strings.Contains(out, "[redacted]") {
		t.Fatalf("a token reached the log: %s", out)
	}
	buf.Reset()
	debug := setup(&buf, "text", "debug", "test")
	Engine(debug).WithValues("password", "hunter2").Info("step")
	if !strings.Contains(buf.String(), "step") || strings.Contains(buf.String(), "hunter2") {
		t.Fatalf("debug shows engine info without secrets: %s", buf.String())
	}
}
