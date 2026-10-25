// Package github reads a source repository the way the other SDI services
// do (CONTRACT.md §18, §24.3): with the workspace's token from
// Identity.GetInstallationToken and its host's API base (GitHub Enterprise
// included), resolving a revision to a commit and downloading the tarball
// at that commit into a temporary directory, within limits.
package github

import (
	"archive/tar"
	"compress/gzip"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/google/uuid"
)

// Access is one workspace's way into GitHub. The token is never logged.
type Access struct {
	Token   string
	APIBase string
}

func (a Access) String() string { return "github.Access{" + a.APIBase + "}" }

// TokenSource mints a workspace's access.
type TokenSource interface {
	Access(ctx context.Context, workspace uuid.UUID) (Access, error)
}

// Limits bound one checkout (§24.3).
type Limits struct {
	MaxTarballBytes int64
	MaxFileBytes    int64
	// MaxExtractedBytes and MaxFiles bound what a small tarball may unpack to.
	MaxExtractedBytes int64
	MaxFiles          int
}

// DefaultLimits are the contract's: tarball 200 MB, 10 MB per file.
var DefaultLimits = Limits{
	MaxTarballBytes:   200 << 20,
	MaxFileBytes:      10 << 20,
	MaxExtractedBytes: 1 << 30,
	MaxFiles:          100_000,
}

const (
	requestTimeout = 30 * time.Second
	tarballTimeout = 5 * time.Minute
	userAgent      = "sdi-gitops"
)

// Error is a GitHub failure as a sentence (it becomes a condition).
type Error struct {
	Message string
	// NotFound is a repository or revision that does not exist (or the
	// token cannot see).
	NotFound bool
}

func (e *Error) Error() string { return e.Message }

// Client talks to GitHub's REST API.
type Client struct {
	http   *http.Client
	tokens TokenSource
	limits Limits
}

// New is a client minting tokens from tokens.
func New(tokens TokenSource, limits Limits) *Client {
	return &Client{
		http: &http.Client{
			// A transport of its own: the process-wide default transport is
			// closed to rendering (render.BlockDefaultNetwork).
			Transport: &http.Transport{
				Proxy:                 http.ProxyFromEnvironment,
				DialContext:           (&net.Dialer{Timeout: 10 * time.Second, KeepAlive: 30 * time.Second}).DialContext,
				ForceAttemptHTTP2:     true,
				MaxIdleConns:          64,
				IdleConnTimeout:       90 * time.Second,
				TLSHandshakeTimeout:   10 * time.Second,
				ExpectContinueTimeout: time.Second,
			},
			// The tarball endpoint answers 302 to codeload; Go drops the
			// Authorization header when a redirect changes host, which is
			// what GitHub expects.
			CheckRedirect: func(req *http.Request, via []*http.Request) error {
				if len(via) >= 5 {
					return errors.New("too many redirects")
				}
				return nil
			},
		},
		tokens: tokens,
		limits: limits,
	}
}

var repoName = regexp.MustCompile(`^[A-Za-z0-9_.-]{1,100}/[A-Za-z0-9_.-]{1,100}$`)

// ValidRepo is `owner/name`.
func ValidRepo(repo string) bool {
	return repoName.MatchString(repo) && !strings.Contains(repo, "..")
}

var fullSHA = regexp.MustCompile(`^[0-9a-f]{40}$`)

func (c *Client) get(ctx context.Context, access Access, p string, accept string, timeout time.Duration) (*http.Response, context.CancelFunc, error) {
	ctx, cancel := context.WithTimeout(ctx, timeout)
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, access.APIBase+p, nil)
	if err != nil {
		cancel()
		return nil, nil, &Error{Message: "the GitHub API address is not valid"}
	}
	req.Header.Set("Accept", accept)
	req.Header.Set("X-GitHub-Api-Version", "2022-11-28")
	req.Header.Set("User-Agent", userAgent)
	if access.Token != "" {
		req.Header.Set("Authorization", "Bearer "+access.Token)
	}
	resp, err := c.http.Do(req)
	if err != nil {
		cancel()
		return nil, nil, &Error{Message: "GitHub could not be reached: " + transportError(err)}
	}
	if resp.StatusCode >= 300 {
		defer cancel()
		defer resp.Body.Close()
		return nil, nil, statusError(resp, p)
	}
	return resp, cancel, nil
}

func statusError(resp *http.Response, p string) error {
	_, _ = io.Copy(io.Discard, io.LimitReader(resp.Body, 64<<10))
	switch resp.StatusCode {
	case http.StatusUnauthorized:
		return &Error{Message: "GitHub rejected the workspace's token"}
	case http.StatusNotFound:
		return &Error{Message: "GitHub has no " + p + " the workspace can see", NotFound: true}
	case http.StatusConflict:
		return &Error{Message: "the repository has no commits yet"}
	case http.StatusForbidden, http.StatusTooManyRequests:
		return &Error{Message: "GitHub refused the request (rate limit or missing permission)"}
	case http.StatusUnprocessableEntity:
		return &Error{Message: "GitHub does not know that revision", NotFound: true}
	}
	return &Error{Message: fmt.Sprintf("GitHub answered %d", resp.StatusCode)}
}

// transportError is a transport failure without the URL (a GHES base may
// carry nothing secret, but redirect targets carry signed query strings).
func transportError(err error) string {
	var uerr *url.Error
	if errors.As(err, &uerr) {
		err = uerr.Err
	}
	msg := err.Error()
	if len(msg) > 200 {
		msg = msg[:200]
	}
	return msg
}

// DefaultBranch is the repository's default branch.
func (c *Client) DefaultBranch(ctx context.Context, workspace uuid.UUID, repo string) (string, error) {
	if !ValidRepo(repo) {
		return "", &Error{Message: fmt.Sprintf("%q is not owner/name", repo)}
	}
	access, err := c.tokens.Access(ctx, workspace)
	if err != nil {
		return "", err
	}
	resp, cancel, err := c.get(ctx, access, "/repos/"+repo, "application/vnd.github+json", requestTimeout)
	if err != nil {
		return "", err
	}
	defer cancel()
	defer resp.Body.Close()
	var body struct {
		DefaultBranch string `json:"default_branch"`
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, 1<<20)).Decode(&body); err != nil || body.DefaultBranch == "" {
		return "", &Error{Message: "GitHub's answer for " + repo + " has no default branch"}
	}
	return body.DefaultBranch, nil
}

// ResolveCommit is the commit SHA a branch, tag or SHA names.
func (c *Client) ResolveCommit(ctx context.Context, workspace uuid.UUID, repo, revision string) (string, error) {
	if !ValidRepo(repo) {
		return "", &Error{Message: fmt.Sprintf("%q is not owner/name", repo)}
	}
	revision = strings.TrimSpace(revision)
	if revision == "" {
		return "", &Error{Message: "no revision to resolve"}
	}
	if fullSHA.MatchString(revision) {
		return revision, nil
	}
	access, err := c.tokens.Access(ctx, workspace)
	if err != nil {
		return "", err
	}
	resp, cancel, err := c.get(ctx, access, "/repos/"+repo+"/commits/"+url.PathEscape(revision), "application/vnd.github+json", requestTimeout)
	if err != nil {
		var gerr *Error
		if errors.As(err, &gerr) && gerr.NotFound {
			return "", &Error{Message: fmt.Sprintf("%s has no branch, tag or commit %q", repo, revision), NotFound: true}
		}
		return "", err
	}
	defer cancel()
	defer resp.Body.Close()
	var body struct {
		SHA string `json:"sha"`
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, 4<<20)).Decode(&body); err != nil || !fullSHA.MatchString(body.SHA) {
		return "", &Error{Message: "GitHub's answer for " + revision + " has no commit SHA"}
	}
	return body.SHA, nil
}

// Checkout is a repository extracted at one commit. Remove deletes it.
type Checkout struct {
	Dir string
	// Oversized are repository paths left out for exceeding the file limit.
	Oversized []string
}

// Remove deletes the checkout.
func (c *Checkout) Remove() {
	if c != nil && c.Dir != "" {
		_ = os.RemoveAll(c.Dir)
	}
}

// Download extracts the repository at sha into a new temporary directory.
func (c *Client) Download(ctx context.Context, workspace uuid.UUID, repo, sha string) (*Checkout, error) {
	if !ValidRepo(repo) {
		return nil, &Error{Message: fmt.Sprintf("%q is not owner/name", repo)}
	}
	if !fullSHA.MatchString(sha) {
		return nil, &Error{Message: "a checkout needs a full commit SHA"}
	}
	access, err := c.tokens.Access(ctx, workspace)
	if err != nil {
		return nil, err
	}
	resp, cancel, err := c.get(ctx, access, "/repos/"+repo+"/tarball/"+sha, "application/vnd.github+json", tarballTimeout)
	if err != nil {
		return nil, err
	}
	defer cancel()
	defer resp.Body.Close()
	if resp.ContentLength > c.limits.MaxTarballBytes {
		return nil, &Error{Message: fmt.Sprintf("the repository tarball is larger than %d MB, the limit", c.limits.MaxTarballBytes>>20)}
	}
	dir, err := os.MkdirTemp("", "sdi-gitops-src-")
	if err != nil {
		return nil, fmt.Errorf("temporary directory: %w", err)
	}
	checkout := &Checkout{Dir: dir}
	if err := Extract(resp.Body, dir, c.limits, checkout); err != nil {
		checkout.Remove()
		return nil, err
	}
	return checkout, nil
}

// countingReader fails once more than max bytes were read.
type countingReader struct {
	r    io.Reader
	n    int64
	max  int64
	over bool
}

func (c *countingReader) Read(p []byte) (int, error) {
	n, err := c.r.Read(p)
	c.n += int64(n)
	if c.n > c.max {
		c.over = true
		return n, errors.New("tarball limit exceeded")
	}
	return n, err
}

// Extract unpacks a GitHub tarball (one top-level directory, stripped) into
// dir: regular files and directories only, nothing outside dir, every file
// at most MaxFileBytes (larger ones are listed in Oversized and left out),
// and the whole within MaxTarballBytes compressed, MaxExtractedBytes and
// MaxFiles unpacked.
func Extract(r io.Reader, dir string, limits Limits, out *Checkout) error {
	counted := &countingReader{r: r, max: limits.MaxTarballBytes}
	tooBig := func() error {
		return &Error{Message: fmt.Sprintf("the repository tarball is larger than %d MB, the limit", limits.MaxTarballBytes>>20)}
	}
	gz, err := gzip.NewReader(counted)
	if err != nil {
		if counted.over {
			return tooBig()
		}
		return &Error{Message: "the repository tarball is not a gzip archive"}
	}
	defer gz.Close()
	tr := tar.NewReader(gz)
	var extracted int64
	files := 0
	for {
		hdr, err := tr.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			if counted.over {
				return tooBig()
			}
			return &Error{Message: "the repository tarball is damaged"}
		}
		rel, ok := stripTop(hdr.Name)
		if !ok {
			continue
		}
		target := filepath.Join(dir, filepath.FromSlash(rel))
		switch hdr.Typeflag {
		case tar.TypeDir:
			if err := os.MkdirAll(target, 0o755); err != nil {
				return fmt.Errorf("extract: %w", err)
			}
		case tar.TypeReg:
			files++
			if files > limits.MaxFiles {
				return &Error{Message: fmt.Sprintf("the repository has more than %d files, the limit", limits.MaxFiles)}
			}
			if hdr.Size > limits.MaxFileBytes {
				out.Oversized = append(out.Oversized, rel)
				continue
			}
			extracted += hdr.Size
			if extracted > limits.MaxExtractedBytes {
				return &Error{Message: fmt.Sprintf("the repository unpacks to more than %d MB, the limit", limits.MaxExtractedBytes>>20)}
			}
			if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
				return fmt.Errorf("extract: %w", err)
			}
			if err := writeFile(target, tr, hdr.Size); err != nil {
				if counted.over {
					return tooBig()
				}
				return err
			}
		default:
			// Symlinks, hard links and devices are left out: a link could
			// point outside the checkout.
		}
	}
	// Drain what is left so a tarball over the limit is noticed even when
	// the excess is trailing padding.
	if _, err := io.Copy(io.Discard, counted); err != nil && counted.over {
		return tooBig()
	}
	return nil
}

func writeFile(target string, r io.Reader, size int64) error {
	f, err := os.OpenFile(target, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o644)
	if err != nil {
		return fmt.Errorf("extract: %w", err)
	}
	_, err = io.Copy(f, io.LimitReader(r, size))
	if cerr := f.Close(); err == nil {
		err = cerr
	}
	if err != nil {
		return &Error{Message: "the repository tarball is damaged"}
	}
	return nil
}

// stripTop drops the archive's top-level directory and refuses anything
// that is not a plain relative path.
func stripTop(name string) (string, bool) {
	name = strings.TrimPrefix(name, "./")
	_, rest, ok := strings.Cut(name, "/")
	if !ok || rest == "" {
		return "", false
	}
	if strings.Contains(rest, "\\") || strings.Contains(rest, "\x00") || strings.HasPrefix(rest, "/") {
		return "", false
	}
	cleaned := path.Clean(rest)
	if cleaned == "." || cleaned == ".." || strings.HasPrefix(cleaned, "../") {
		return "", false
	}
	return cleaned, true
}
