package render

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"

	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
	"sigs.k8s.io/kustomize/api/krusty"
	"sigs.k8s.io/kustomize/api/types"
	"sigs.k8s.io/kustomize/kyaml/filesys"
	"sigs.k8s.io/yaml"
)

var kustomizeMu sync.Mutex

// renderKustomize builds the kustomization in dir with the kustomize Go API.
// Loading is restricted to the checkout: a confined file system refuses any
// path outside root, and every kustomization file is checked for remote
// references before kustomize sees it, so no base, component or resource
// is ever cloned or downloaded. Image overrides are applied the way
// `kustomize edit set image` (Argo CD's spec.source.kustomize.images) does:
// written into the kustomization's images list, merged by name.
func renderKustomize(root, dir string, images []string) ([]*unstructured.Unstructured, error) {
	// kustomize keeps process-wide OpenAPI state (kyaml's openapi.SetSchema)
	// that concurrent builds would race on; Flux serialises its builds for
	// the same reason. A build takes milliseconds.
	kustomizeMu.Lock()
	defer kustomizeMu.Unlock()
	if len(images) > 0 {
		if err := setImages(dir, images); err != nil {
			return nil, err
		}
	}
	opts := krusty.MakeDefaultOptions()
	// Bases may live anywhere in the repository (../base); the confined
	// file system is what keeps them inside it.
	opts.LoadRestrictions = types.LoadRestrictionsNone
	opts.PluginConfig = types.DisabledPluginConfig()
	fsys := &confinedFS{root: root, disk: filesys.MakeFsOnDisk()}
	resMap, err := krusty.MakeKustomizer(opts).Run(fsys, dir)
	if err != nil {
		if fsys.refusal != nil {
			return nil, fsys.refusal
		}
		return nil, errorf("kustomize build failed: %s", relMessage(root, err.Error()))
	}
	raw, err := resMap.AsYaml()
	if err != nil {
		return nil, errorf("kustomize output could not be serialised: %s", shortError(err))
	}
	objs, err := documents("kustomize output", ".yaml", raw)
	if err != nil {
		return nil, err
	}
	return objs, nil
}

// relMessage keeps the checkout's temporary location out of messages.
func relMessage(root, msg string) string {
	msg = strings.ReplaceAll(msg, root+string(filepath.Separator), "")
	msg = strings.ReplaceAll(msg, root, ".")
	if len(msg) > 600 {
		msg = msg[:600] + "..."
	}
	return msg
}

// --- Remote references ---------------------------------------------------------

var scpStyle = regexp.MustCompile(`^[a-zA-Z][a-zA-Z0-9-]*@`)

// isRemote is whether kustomize would fetch ref over the network: the
// shapes its loader treats as remote (internal/git.NewRepoSpecFromURL and
// loader.IsRemoteFile), and file:// for good measure.
func isRemote(ref string) bool {
	ref = strings.TrimSpace(ref)
	lower := strings.ToLower(ref)
	return strings.HasPrefix(lower, "git::") ||
		strings.Contains(lower, "://") ||
		scpStyle.MatchString(ref) ||
		strings.HasPrefix(lower, "github.com/") ||
		strings.HasPrefix(lower, "github.com:")
}

// checkKustomization refuses a kustomization that references anything
// remote, or uses features that would run something.
func checkKustomization(name string, data []byte) error {
	var k map[string]any
	if err := yaml.Unmarshal(data, &k); err != nil {
		// kustomize itself reports the syntax error with more context.
		return nil
	}
	for _, key := range []string{"helmCharts", "helmChartInflationGenerator", "helmGlobals"} {
		if _, ok := k[key]; ok {
			return errorf("%s uses %s; Helm charts inside kustomize are not supported, use a helm source", name, key)
		}
	}
	refuse := func(field, ref string) error {
		return errorf("%s references %q in %s; remote kustomize bases and resources are not supported, vendor them into the repository", name, ref, field)
	}
	strs := func(v any) []string {
		list, _ := v.([]any)
		out := make([]string, 0, len(list))
		for _, item := range list {
			if s, ok := item.(string); ok {
				out = append(out, s)
			}
		}
		return out
	}
	for _, field := range []string{"resources", "bases", "components", "crds", "configurations",
		"generators", "transformers", "validators", "patchesStrategicMerge"} {
		for _, ref := range strs(k[field]) {
			// Inline patches and plugin configs span lines; only a one-line
			// entry is a reference.
			if !strings.Contains(ref, "\n") && isRemote(ref) {
				return refuse(field, ref)
			}
		}
	}
	for _, field := range []string{"patches", "patchesJson6902", "replacements"} {
		list, _ := k[field].([]any)
		for _, item := range list {
			m, _ := item.(map[string]any)
			if p, ok := m["path"].(string); ok && isRemote(p) {
				return refuse(field, p)
			}
		}
	}
	if m, ok := k["openapi"].(map[string]any); ok {
		if p, ok := m["path"].(string); ok && isRemote(p) {
			return refuse("openapi", p)
		}
	}
	for _, field := range []string{"configMapGenerator", "secretGenerator"} {
		list, _ := k[field].([]any)
		for _, item := range list {
			m, _ := item.(map[string]any)
			refs := append(strs(m["files"]), strs(m["envs"])...)
			if env, ok := m["env"].(string); ok {
				refs = append(refs, env)
			}
			for _, ref := range refs {
				// files entries may be key=path.
				if _, after, ok := strings.Cut(ref, "="); ok {
					ref = after
				}
				if isRemote(ref) {
					return refuse(field, ref)
				}
			}
		}
	}
	return nil
}

// --- Confined file system --------------------------------------------------------

// confinedFS is the on-disk file system restricted to one directory tree,
// read-only, that checks every kustomization file it hands out.
type confinedFS struct {
	root    string
	disk    filesys.FileSystem
	refusal error
}

var _ filesys.FileSystem = (*confinedFS)(nil)

func (f *confinedFS) inside(p string) (string, error) {
	if !filepath.IsAbs(p) {
		p = filepath.Join(f.root, p)
	}
	p = filepath.Clean(p)
	if p != f.root && !strings.HasPrefix(p, f.root+string(filepath.Separator)) {
		err := errorf("kustomize tried to read %s, outside the repository", relMessage(f.root, p))
		if f.refusal == nil {
			f.refusal = err
		}
		return "", err
	}
	return p, nil
}

func (f *confinedFS) readOnly() error {
	return fmt.Errorf("the checkout is read-only")
}

func (f *confinedFS) Create(string) (filesys.File, error) { return nil, f.readOnly() }
func (f *confinedFS) Mkdir(string) error                  { return f.readOnly() }
func (f *confinedFS) MkdirAll(string) error               { return f.readOnly() }
func (f *confinedFS) RemoveAll(string) error              { return f.readOnly() }
func (f *confinedFS) WriteFile(string, []byte) error      { return f.readOnly() }

func (f *confinedFS) Open(p string) (filesys.File, error) {
	full, err := f.inside(p)
	if err != nil {
		return nil, err
	}
	if isKustomizationName(full) {
		if _, err := f.ReadFile(full); err != nil {
			return nil, err
		}
	}
	return f.disk.Open(full)
}

func (f *confinedFS) IsDir(p string) bool {
	full, err := f.inside(p)
	return err == nil && f.disk.IsDir(full)
}

func (f *confinedFS) ReadDir(p string) ([]string, error) {
	full, err := f.inside(p)
	if err != nil {
		return nil, err
	}
	return f.disk.ReadDir(full)
}

func (f *confinedFS) CleanedAbs(p string) (filesys.ConfirmedDir, string, error) {
	full, err := f.inside(p)
	if err != nil {
		return "", "", err
	}
	dir, file, err := f.disk.CleanedAbs(full)
	if err != nil {
		return "", "", err
	}
	if _, err := f.inside(dir.String()); err != nil {
		return "", "", err
	}
	return dir, file, nil
}

func (f *confinedFS) Exists(p string) bool {
	full, err := f.inside(p)
	return err == nil && f.disk.Exists(full)
}

func (f *confinedFS) Glob(pattern string) ([]string, error) {
	full, err := f.inside(pattern)
	if err != nil {
		return nil, err
	}
	matches, err := f.disk.Glob(full)
	if err != nil {
		return nil, err
	}
	out := matches[:0]
	for _, m := range matches {
		if _, err := f.inside(m); err == nil {
			out = append(out, m)
		}
	}
	return out, nil
}

func (f *confinedFS) ReadFile(p string) ([]byte, error) {
	full, err := f.inside(p)
	if err != nil {
		return nil, err
	}
	data, err := f.disk.ReadFile(full)
	if err != nil {
		return nil, err
	}
	if isKustomizationName(full) {
		rel, _ := filepath.Rel(f.root, full)
		if err := checkKustomization(filepath.ToSlash(rel), data); err != nil {
			if f.refusal == nil {
				f.refusal = err
			}
			return nil, err
		}
	}
	return data, nil
}

func (f *confinedFS) Walk(p string, walkFn filepath.WalkFunc) error {
	full, err := f.inside(p)
	if err != nil {
		return err
	}
	return f.disk.Walk(full, walkFn)
}

func isKustomizationName(p string) bool {
	base := filepath.Base(p)
	for _, name := range kustomizationNames {
		if base == name {
			return true
		}
	}
	return false
}

// --- Image overrides ---------------------------------------------------------------

// Image is one kustomize image override.
type Image struct {
	Name    string
	NewName string
	NewTag  string
	Digest  string
}

// ParseImage reads an override the way `kustomize edit set image` does:
// `name=newName:tag`, `name=newName@sha256:…`, `name:tag`, `name@sha256:…`
// or `name=newName`.
func ParseImage(raw string) (Image, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" || strings.ContainsAny(raw, " \t\n") {
		return Image{}, fmt.Errorf("image override %q is not name=newName:tag, name:tag or name@digest", raw)
	}
	name, image, hasNew := strings.Cut(raw, "=")
	if !hasNew {
		image = raw
	}
	repo, tag, digest := splitImage(image)
	if repo == "" || (hasNew && name == "") {
		return Image{}, fmt.Errorf("image override %q is not name=newName:tag, name:tag or name@digest", raw)
	}
	if digest != "" && !strings.HasPrefix(digest, "sha256:") {
		return Image{}, fmt.Errorf("image override %q has a digest that is not sha256:…", raw)
	}
	img := Image{NewTag: tag, Digest: digest}
	if hasNew {
		img.Name = name
		if repo != name {
			img.NewName = repo
		}
	} else {
		img.Name = repo
		if tag == "" && digest == "" {
			return Image{}, fmt.Errorf("image override %q changes nothing; give a tag, a digest or name=newName", raw)
		}
	}
	return img, nil
}

// splitImage splits repo[:tag][@digest]; a colon before the last slash is a
// registry port, not a tag.
func splitImage(s string) (repo, tag, digest string) {
	if i := strings.Index(s, "@"); i >= 0 {
		s, digest = s[:i], s[i+1:]
	}
	if i := strings.LastIndex(s, ":"); i >= 0 && i > strings.LastIndex(s, "/") {
		s, tag = s[:i], s[i+1:]
	}
	return s, tag, digest
}

// setImages merges overrides into the kustomization in dir, by image name.
func setImages(dir string, overrides []string) error {
	var file string
	for _, name := range kustomizationNames {
		if isFile(filepath.Join(dir, name)) {
			file = filepath.Join(dir, name)
			break
		}
	}
	if file == "" {
		return errorf("image overrides need a kustomization in the source path")
	}
	data, err := os.ReadFile(file)
	if err != nil {
		return errorf("the kustomization could not be read")
	}
	var k map[string]any
	if err := yaml.Unmarshal(data, &k); err != nil || k == nil {
		return errorf("the kustomization is not valid YAML: %s", shortError(err))
	}
	existing, _ := k["images"].([]any)
	byName := map[string]int{}
	for i, item := range existing {
		if m, ok := item.(map[string]any); ok {
			if n, ok := m["name"].(string); ok {
				byName[n] = i
			}
		}
	}
	for _, raw := range overrides {
		img, err := ParseImage(raw)
		if err != nil {
			return &Error{Message: err.Error()}
		}
		entry := map[string]any{"name": img.Name}
		if i, ok := byName[img.Name]; ok {
			if m, ok := existing[i].(map[string]any); ok {
				entry = m
			}
		}
		if img.NewName != "" {
			entry["newName"] = img.NewName
		}
		if img.NewTag != "" {
			entry["newTag"] = img.NewTag
			delete(entry, "digest")
		}
		if img.Digest != "" {
			entry["digest"] = img.Digest
			delete(entry, "newTag")
		}
		if i, ok := byName[img.Name]; ok {
			existing[i] = entry
		} else {
			byName[img.Name] = len(existing)
			existing = append(existing, entry)
		}
	}
	k["images"] = existing
	out, err := yaml.Marshal(k)
	if err != nil {
		return errorf("the kustomization could not be written")
	}
	if err := os.WriteFile(file, out, 0o600); err != nil {
		return errorf("the kustomization could not be written")
	}
	return nil
}
