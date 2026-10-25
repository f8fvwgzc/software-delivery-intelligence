package render

import (
	"fmt"
	"os"
	"strings"

	"helm.sh/helm/v3/pkg/action"
	"helm.sh/helm/v3/pkg/chart/loader"
	"helm.sh/helm/v3/pkg/chartutil"
	"helm.sh/helm/v3/pkg/strvals"
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
	"sigs.k8s.io/yaml"
)

// renderHelm templates the chart in dir with the Helm Go SDK, client-only
// (what `helm template` does, which is what Argo CD runs): values files in
// order, then parameters, the release name and the destination namespace.
// Hooks are included, as `helm template` prints them, and the engine maps
// Helm's hook annotations onto sync phases. Nothing is fetched: chart
// dependencies must be vendored under charts/, the `lookup` function sees an
// empty cluster and DNS lookups are off.
func renderHelm(root, dir string, req Request) ([]*unstructured.Unstructured, error) {
	chrt, err := loader.LoadDir(dir)
	if err != nil {
		return nil, errorf("the Helm chart could not be loaded: %s", relMessage(root, shortError(err)))
	}
	if deps := chrt.Metadata.Dependencies; len(deps) > 0 {
		if err := action.CheckDependencies(chrt, deps); err != nil {
			return nil, errorf("chart dependencies must be vendored under charts/ (remote dependencies are not supported): %s", shortError(err))
		}
	}
	values, err := helmValues(root, dir, req.HelmValueFiles, req.HelmParameters)
	if err != nil {
		return nil, err
	}
	install := action.NewInstall(&action.Configuration{Log: func(string, ...any) {}})
	install.DryRun = true
	install.ClientOnly = true
	install.Replace = true
	install.IncludeCRDs = true
	install.EnableDNS = false
	install.ReleaseName = req.HelmReleaseName
	install.Namespace = req.Namespace
	if req.KubeVersion != "" {
		if kv, err := chartutil.ParseKubeVersion(req.KubeVersion); err == nil {
			install.KubeVersion = kv
		}
	}
	install.APIVersions = chartutil.VersionSet(req.APIVersions)
	rel, err := install.Run(chrt, values)
	if err != nil {
		return nil, errorf("helm template failed: %s", relMessage(root, shortError(err)))
	}
	var b strings.Builder
	b.WriteString(rel.Manifest)
	for _, hook := range rel.Hooks {
		b.WriteString("\n---\n")
		b.WriteString(hook.Manifest)
	}
	return documents("the chart's output", ".yaml", []byte(b.String()))
}

// helmValues merges the values files (relative to the chart, inside the
// repository) in order, then the parameters as `--set` does.
func helmValues(root, dir string, files []string, params map[string]string) (map[string]any, error) {
	values := map[string]any{}
	for _, f := range files {
		full, err := insideRoot(root, dir, f)
		if err != nil {
			return nil, errorf("values file %s", err.Error())
		}
		data, err := os.ReadFile(full)
		if err != nil {
			return nil, errorf("values file %s is not in the repository", f)
		}
		var current map[string]any
		if err := yaml.Unmarshal(data, &current); err != nil {
			return nil, errorf("values file %s is not valid YAML: %s", f, shortError(err))
		}
		values = mergeValues(values, current)
	}
	for _, key := range sortedKeys(params) {
		if strings.TrimSpace(key) == "" {
			return nil, errorf("a helm parameter has an empty name")
		}
		if err := strvals.ParseInto(key+"="+escapeSetValue(params[key]), values); err != nil {
			return nil, errorf("helm parameter %s is not valid: %s", key, shortError(err))
		}
	}
	return values, nil
}

// escapeSetValue makes a value literal for --set: commas and backslashes
// would otherwise split or escape it.
func escapeSetValue(v string) string {
	v = strings.ReplaceAll(v, `\`, `\\`)
	return strings.ReplaceAll(v, ",", `\,`)
}

// mergeValues is Helm's merge of successive -f files: maps merge key by
// key, anything else is replaced.
func mergeValues(a, b map[string]any) map[string]any {
	out := make(map[string]any, len(a))
	for k, v := range a {
		out[k] = v
	}
	for k, v := range b {
		if vm, ok := v.(map[string]any); ok {
			if bv, ok := out[k].(map[string]any); ok {
				out[k] = mergeValues(bv, vm)
				continue
			}
		}
		out[k] = v
	}
	return out
}

// ValidReleaseName is Helm's rule for release names.
func ValidReleaseName(name string) error {
	if err := chartutil.ValidateReleaseName(name); err != nil {
		return fmt.Errorf("helm release name %q is not valid: lowercase letters, digits, '-' and '.', at most 53 characters", name)
	}
	return nil
}
