package render

import (
	"context"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
)

// repo writes files (path → content) into a fresh checkout directory.
func repo(t *testing.T, files map[string]string) string {
	t.Helper()
	root := t.TempDir()
	for name, content := range files {
		p := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	return root
}

func run(t *testing.T, req Request) (Result, error) {
	t.Helper()
	return Render(context.Background(), req, DefaultLimits)
}

func mustRender(t *testing.T, req Request) Result {
	t.Helper()
	res, err := run(t, req)
	if err != nil {
		t.Fatalf("render: %v", err)
	}
	return res
}

func byKindName(objs []*unstructured.Unstructured) map[string]*unstructured.Unstructured {
	out := map[string]*unstructured.Unstructured{}
	for _, o := range objs {
		out[o.GetKind()+"/"+o.GetName()] = o
	}
	return out
}

func wantError(t *testing.T, err error, fragment string) {
	t.Helper()
	if err == nil {
		t.Fatalf("expected an error containing %q", fragment)
	}
	if _, ok := AsError(err); !ok {
		t.Fatalf("expected a render error, got %T: %v", err, err)
	}
	if !strings.Contains(err.Error(), fragment) {
		t.Fatalf("error %q does not contain %q", err.Error(), fragment)
	}
}

const configMap = `apiVersion: v1
kind: ConfigMap
metadata:
  name: %s
data:
  key: value
`

func cm(name string) string { return strings.Replace(configMap, "%s", name, 1) }

func TestDirectoryReadsYAMLAndJSONAndSkipsOtherYAML(t *testing.T) {
	root := repo(t, map[string]string{
		"deploy/a.yaml":          cm("a") + "---\n" + cm("b"),
		"deploy/c.json":          `{"apiVersion":"v1","kind":"ConfigMap","metadata":{"name":"c"}}`,
		"deploy/list.yml":        "apiVersion: v1\nkind: List\nitems:\n- apiVersion: v1\n  kind: ConfigMap\n  metadata:\n    name: d\n",
		"deploy/values.yaml":     "replicas: 3\nimage: nginx\n",
		"deploy/package.json":    `{"name":"not-a-manifest"}`,
		"deploy/README.md":       "# not read",
		"deploy/nested/e.yaml":   cm("e"),
		"elsewhere/ignored.yaml": cm("ignored"),
	})
	res := mustRender(t, Request{Root: root, Path: "deploy"})
	if res.Type != TypeDirectory {
		t.Fatalf("type = %s", res.Type)
	}
	got := byKindName(res.Objects)
	for _, name := range []string{"a", "b", "c", "d"} {
		if got["ConfigMap/"+name] == nil {
			t.Errorf("ConfigMap %s missing; got %v", name, keys(got))
		}
	}
	if got["ConfigMap/e"] != nil || got["ConfigMap/ignored"] != nil {
		t.Errorf("subdirectories or other paths were read without recurse: %v", keys(got))
	}
	if len(res.Objects) != 4 {
		t.Errorf("got %d objects, want 4: %v", len(res.Objects), keys(got))
	}

	res = mustRender(t, Request{Root: root, Path: "deploy", Recurse: true})
	if byKindName(res.Objects)["ConfigMap/e"] == nil {
		t.Errorf("recurse did not read the nested directory")
	}
}

func keys(m map[string]*unstructured.Unstructured) []string {
	var out []string
	for k := range m {
		out = append(out, k)
	}
	return out
}

func TestDirectoryRefusesBrokenManifests(t *testing.T) {
	root := repo(t, map[string]string{
		"broken.yaml": "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: x\n  labels: [unclosed\n",
	})
	_, err := run(t, Request{Root: root})
	wantError(t, err, "broken.yaml")

	root = repo(t, map[string]string{"two.json": `{"apiVersion":"v1","kind":"ConfigMap","metadata":{"name":"a"}}{"apiVersion":"v1","kind":"ConfigMap","metadata":{"name":"b"}}`})
	_, err = run(t, Request{Root: root})
	wantError(t, err, "more than one JSON object")
}

func TestDetection(t *testing.T) {
	root := repo(t, map[string]string{
		"k/kustomization.yaml": "resources: [cm.yaml]\n",
		"k/cm.yaml":            cm("k"),
		"h/Chart.yaml":         "apiVersion: v2\nname: h\nversion: 0.1.0\n",
		"h/templates/cm.yaml":  cm("h"),
		"d/cm.yaml":            cm("d"),
	})
	for path, want := range map[string]string{"k": TypeKustomize, "h": TypeHelm, "d": TypeDirectory} {
		if got := Detect(filepath.Join(root, path)); got != want {
			t.Errorf("Detect(%s) = %s, want %s", path, got, want)
		}
		res := mustRender(t, Request{Root: root, Path: path, Namespace: "ns", HelmReleaseName: "rel"})
		if res.Type != want || len(res.Objects) != 1 {
			t.Errorf("%s rendered as %s with %d objects", path, res.Type, len(res.Objects))
		}
	}
}

const deployment = `apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
spec:
  selector:
    matchLabels: {app: web}
  template:
    metadata:
      labels: {app: web}
    spec:
      containers:
      - name: web
        image: placeholder
      - name: sidecar
        image: registry.example.com:5000/tools/sidecar:1.0
`

func containerImages(t *testing.T, obj *unstructured.Unstructured) []string {
	t.Helper()
	containers, _, _ := unstructured.NestedSlice(obj.Object, "spec", "template", "spec", "containers")
	var out []string
	for _, c := range containers {
		out = append(out, c.(map[string]any)["image"].(string))
	}
	return out
}

func TestKustomizeWithBaseOutsideThePathAndImageOverrides(t *testing.T) {
	root := repo(t, map[string]string{
		"base/kustomization.yaml":         "resources: [deployment.yaml]\n",
		"base/deployment.yaml":            deployment,
		"overlays/prod/kustomization.yml": "resources: [../../base]\nnamePrefix: prod-\nimages:\n- name: registry.example.com:5000/tools/sidecar\n  newTag: \"1.1\"\n",
	})
	res := mustRender(t, Request{Root: root, Path: "overlays/prod", KustomizeImages: []string{
		"placeholder=registry.k8s.io/pause:3.10",
		"registry.example.com:5000/tools/sidecar:2.0",
	}})
	if res.Type != TypeKustomize || len(res.Objects) != 1 {
		t.Fatalf("type %s, %d objects", res.Type, len(res.Objects))
	}
	d := res.Objects[0]
	if d.GetName() != "prod-web" {
		t.Errorf("name = %s", d.GetName())
	}
	images := containerImages(t, d)
	if images[0] != "registry.k8s.io/pause:3.10" {
		t.Errorf("override not applied: %v", images)
	}
	// The override replaced the overlay's own entry for the same image.
	if images[1] != "registry.example.com:5000/tools/sidecar:2.0" {
		t.Errorf("override not merged by name: %v", images)
	}
	// Overrides with a digest.
	res = mustRender(t, Request{Root: root, Path: "overlays/prod", KustomizeImages: []string{"placeholder=repo/app@sha256:" + strings.Repeat("a", 64)}})
	if got := containerImages(t, res.Objects[0])[0]; got != "repo/app@sha256:"+strings.Repeat("a", 64) {
		t.Errorf("digest override: %s", got)
	}
}

func TestKustomizeRefusesRemoteBasesAndPathsOutsideTheCheckout(t *testing.T) {
	cases := map[string]string{
		"github shorthand": "resources:\n- github.com/kubernetes-sigs/kustomize//examples/helloWorld?ref=v3.3.1\n",
		"https base":       "resources:\n- https://github.com/org/repo//base?ref=main\n",
		"ssh base":         "resources:\n- git@github.com:org/repo.git\n",
		"remote file":      "resources:\n- https://raw.githubusercontent.com/org/repo/main/cm.yaml\n",
		"git:: prefix":     "components:\n- git::https://example.com/org/repo\n",
		"remote patch":     "resources: [cm.yaml]\npatches:\n- path: https://example.com/patch.yaml\n",
		"generator file":   "configMapGenerator:\n- name: x\n  files:\n  - key=https://example.com/data.txt\n",
	}
	for name, kustomization := range cases {
		t.Run(name, func(t *testing.T) {
			root := repo(t, map[string]string{"app/kustomization.yaml": kustomization, "app/cm.yaml": cm("a")})
			_, err := run(t, Request{Root: root, Path: "app"})
			wantError(t, err, "remote kustomize bases")
		})
	}
	t.Run("nested remote base", func(t *testing.T) {
		root := repo(t, map[string]string{
			"app/kustomization.yaml":  "resources: [../base]\n",
			"base/kustomization.yaml": "resources:\n- github.com/org/repo/base\n",
		})
		_, err := run(t, Request{Root: root, Path: "app"})
		wantError(t, err, "base/kustomization.yaml")
	})
	t.Run("outside the checkout", func(t *testing.T) {
		outside := t.TempDir()
		if err := os.WriteFile(filepath.Join(outside, "cm.yaml"), []byte(cm("secret")), 0o644); err != nil {
			t.Fatal(err)
		}
		root := repo(t, map[string]string{"app/kustomization.yaml": "resources:\n- " + filepath.Join(outside, "cm.yaml") + "\n"})
		_, err := run(t, Request{Root: root, Path: "app"})
		wantError(t, err, "outside the repository")
		root = repo(t, map[string]string{"app/kustomization.yaml": "resources:\n- ../../../../../../../../../etc\n"})
		_, err = run(t, Request{Root: root, Path: "app"})
		wantError(t, err, "outside the repository")
	})
	t.Run("helm inside kustomize", func(t *testing.T) {
		root := repo(t, map[string]string{"app/kustomization.yaml": "helmCharts:\n- name: x\n  repo: https://charts.example.com\n"})
		_, err := run(t, Request{Root: root, Path: "app"})
		wantError(t, err, "helmCharts")
	})
}

func TestTheDefaultTransportIsClosedToRendering(t *testing.T) {
	saved := http.DefaultTransport
	BlockDefaultNetwork()
	t.Cleanup(func() { http.DefaultTransport = saved })
	_, err := http.Get("http://127.0.0.1:1/")
	if err == nil || !strings.Contains(err.Error(), "network access is not allowed") {
		t.Fatalf("default client reached the network: %v", err)
	}
}

func helmChart() map[string]string {
	return map[string]string{
		"chart/Chart.yaml": `apiVersion: v2
name: web
version: 0.1.0
dependencies:
- name: sub
  version: 0.1.0
`,
		"chart/values.yaml":           "replicaCount: 1\nimage:\n  repository: nginx\n  tag: \"1.0\"\nmessage: default\n",
		"chart/values-prod.yaml":      "replicaCount: 3\nmessage: prod\n",
		"chart/charts/sub/Chart.yaml": "apiVersion: v2\nname: sub\nversion: 0.1.0\n",
		"chart/charts/sub/templates/cm.yaml": `apiVersion: v1
kind: ConfigMap
metadata:
  name: {{ .Release.Name }}-sub
data:
  from: subchart
`,
		"chart/templates/deployment.yaml": `apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ .Release.Name }}
  namespace: {{ .Release.Namespace }}
spec:
  replicas: {{ .Values.replicaCount }}
  selector:
    matchLabels: {app: web}
  template:
    metadata:
      labels: {app: web}
      annotations:
        message: {{ .Values.message | quote }}
        kube: {{ .Capabilities.KubeVersion.Version | quote }}
        lookup: {{ (lookup "v1" "Namespace" "" "kube-system") | len | quote }}
    spec:
      containers:
      - name: web
        image: "{{ .Values.image.repository }}:{{ .Values.image.tag }}"
`,
		"chart/templates/hook.yaml": `apiVersion: batch/v1
kind: Job
metadata:
  name: {{ .Release.Name }}-migrate
  annotations:
    helm.sh/hook: pre-install,pre-upgrade
spec:
  template:
    spec:
      restartPolicy: Never
      containers:
      - name: migrate
        image: busybox
`,
		"chart/templates/NOTES.txt": "not a manifest",
		"shared/values-extra.yaml":  "message: extra\n",
	}
}

func TestHelmValuesParametersReleaseAndHooks(t *testing.T) {
	root := repo(t, helmChart())
	res := mustRender(t, Request{
		Root: root, Path: "chart", HelmReleaseName: "shop", Namespace: "prod",
		HelmValueFiles: []string{"values-prod.yaml", "../shared/values-extra.yaml"},
		HelmParameters: map[string]string{"image.tag": "2.0,beta", "replicaCount": "5"},
		KubeVersion:    "v1.31.0",
	})
	if res.Type != TypeHelm {
		t.Fatalf("type %s", res.Type)
	}
	got := byKindName(res.Objects)
	d := got["Deployment/shop"]
	if d == nil {
		t.Fatalf("deployment missing: %v", keys(got))
	}
	if d.GetNamespace() != "prod" {
		t.Errorf("release namespace = %s", d.GetNamespace())
	}
	replicas, _, _ := unstructured.NestedInt64(d.Object, "spec", "replicas")
	if replicas != 5 {
		t.Errorf("parameters must win over values files: replicas = %d", replicas)
	}
	annotations, _, _ := unstructured.NestedStringMap(d.Object, "spec", "template", "metadata", "annotations")
	if annotations["message"] != "extra" {
		t.Errorf("values files must apply in order: message = %q", annotations["message"])
	}
	if annotations["kube"] != "v1.31.0" {
		t.Errorf("kube version capability = %q", annotations["kube"])
	}
	if annotations["lookup"] != "0" {
		t.Errorf("lookup must see an empty cluster: %q", annotations["lookup"])
	}
	if img := containerImages(t, d)[0]; img != "nginx:2.0,beta" {
		t.Errorf("a parameter value with a comma is taken literally: %s", img)
	}
	if got["ConfigMap/shop-sub"] == nil {
		t.Errorf("vendored subchart not rendered: %v", keys(got))
	}
	hook := got["Job/shop-migrate"]
	if hook == nil || hook.GetAnnotations()["helm.sh/hook"] == "" {
		t.Errorf("hooks must be part of the output, as helm template prints them: %v", keys(got))
	}
}

func TestHelmRefusesMissingDependenciesAndValuesOutsideTheRepository(t *testing.T) {
	files := helmChart()
	delete(files, "chart/charts/sub/Chart.yaml")
	delete(files, "chart/charts/sub/templates/cm.yaml")
	root := repo(t, files)
	_, err := run(t, Request{Root: root, Path: "chart", HelmReleaseName: "shop", Namespace: "ns"})
	wantError(t, err, "vendored under charts/")

	root = repo(t, helmChart())
	for _, bad := range []string{"../../../../etc/passwd", "/etc/passwd", "https://example.com/values.yaml"} {
		_, err := run(t, Request{Root: root, Path: "chart", HelmReleaseName: "shop", Namespace: "ns", HelmValueFiles: []string{bad}})
		wantError(t, err, "values file")
	}
}

func TestLimits(t *testing.T) {
	var many strings.Builder
	for i := 0; i < 12; i++ {
		many.WriteString(cm("cm-" + strings.Repeat("x", i+1)))
		many.WriteString("---\n")
	}
	root := repo(t, map[string]string{"many.yaml": many.String()})
	limits := DefaultLimits
	limits.MaxResources = 10
	_, err := Render(context.Background(), Request{Root: root}, limits)
	wantError(t, err, "the limit")

	root = repo(t, map[string]string{"big.yaml": cm("big") + "  pad: " + strings.Repeat("x", 2048) + "\n"})
	limits = DefaultLimits
	limits.MaxFileBytes = 1024
	_, err = Render(context.Background(), Request{Root: root}, limits)
	wantError(t, err, "larger than")

	// A file the checkout left out for its size fails the render when it is
	// under the source path, and does not matter elsewhere.
	root = repo(t, map[string]string{"app/cm.yaml": cm("a")})
	_, err = run(t, Request{Root: root, Path: "app", Oversized: []string{"app/huge.bin"}})
	wantError(t, err, "app/huge.bin is larger than")
	mustRender(t, Request{Root: root, Path: "app", Oversized: []string{"assets/huge.bin"}})
}

func TestPathsMustStayInTheRepository(t *testing.T) {
	for _, p := range []string{"../x", "/etc", "a/../../b", "a\\b"} {
		if _, err := CleanPath(p); err == nil {
			t.Errorf("CleanPath(%q) accepted", p)
		}
	}
	for in, want := range map[string]string{"": "", ".": "", "a/b/": "a/b", "./a": "a", "a/./b": "a/b"} {
		if got, err := CleanPath(in); err != nil || got != want {
			t.Errorf("CleanPath(%q) = %q, %v", in, got, err)
		}
	}
	root := repo(t, map[string]string{"cm.yaml": cm("a")})
	_, err := run(t, Request{Root: root, Path: "missing"})
	wantError(t, err, "not a directory")
}

func TestParseImage(t *testing.T) {
	cases := map[string]Image{
		"nginx=my/nginx:1.2":                 {Name: "nginx", NewName: "my/nginx", NewTag: "1.2"},
		"nginx:1.2":                          {Name: "nginx", NewTag: "1.2"},
		"nginx=my/nginx":                     {Name: "nginx", NewName: "my/nginx"},
		"localhost:5000/app:v3":              {Name: "localhost:5000/app", NewTag: "v3"},
		"app=localhost:5000/app@sha256:abcd": {Name: "app", NewName: "localhost:5000/app", Digest: "sha256:abcd"},
		"nginx=nginx:1.3":                    {Name: "nginx", NewTag: "1.3"},
	}
	for raw, want := range cases {
		got, err := ParseImage(raw)
		if err != nil || got != want {
			t.Errorf("ParseImage(%q) = %+v, %v; want %+v", raw, got, err, want)
		}
	}
	for _, bad := range []string{"", "nginx", "=x:1", "a b:1", "app@md5:1"} {
		if _, err := ParseImage(bad); err == nil {
			t.Errorf("ParseImage(%q) accepted", bad)
		}
	}
}

func TestReleaseNames(t *testing.T) {
	if err := ValidReleaseName("shop-prod"); err != nil {
		t.Error(err)
	}
	if err := ValidReleaseName("Shop_Prod"); err == nil {
		t.Error("an invalid release name was accepted")
	}
}
