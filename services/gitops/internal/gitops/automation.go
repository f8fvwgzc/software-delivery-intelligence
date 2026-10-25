package gitops

import (
	"fmt"
	"time"

	"github.com/f8fvwgzc/software-delivery-intelligence/services/gitops/internal/store"
)

// Backoff is the wait before retry number n (1-based) of a failed automated
// sync: 10 s, 20 s, 40 s, …, capped at 5 minutes (CONTRACT.md §24.6).
func Backoff(n int) time.Duration {
	if n < 1 {
		n = 1
	}
	d := 10 * time.Second
	for i := 1; i < n; i++ {
		d *= 2
		if d >= 5*time.Minute {
			return 5 * time.Minute
		}
	}
	return d
}

// AutoInput is what the automation decision looks at.
type AutoInput struct {
	Policy store.SyncPolicy
	// Status is the comparison that just completed.
	Status store.Status
	// SyncedDigest is the desired state the last successful sync applied.
	SyncedDigest string
	// AutoDigest, AutoAttempts and AutoNextAt are the failed automated
	// attempts so far and when the next may start.
	AutoDigest   string
	AutoAttempts int
	AutoNextAt   *time.Time
	// OperationRunning is an operation already holding the application.
	OperationRunning bool
	Now              time.Time
}

// AutoDecision is what the loop does.
type AutoDecision struct {
	Sync bool
	// SelfHeal is a sync of the same desired state, undoing drift: only the
	// resources that are not synced, without hooks (as Argo CD self-heals).
	SelfHeal bool
	// Reason explains a decision not to sync (logs, tests).
	Reason string
	// Condition is a sentence for the application's conditions.
	Condition string
	// RetryAt is when a waiting retry may start.
	RetryAt *time.Time
}

// Decide is the automated sync policy of CONTRACT.md §24.6, after Argo
// CD's application controller (autoSync): sync when out of sync, on a new
// rendered state always, on drift of the same state only with self_heal;
// retry a failed attempt with backoff up to retry_limit, then wait for a
// new revision.
func Decide(in AutoInput) AutoDecision {
	switch {
	case !in.Policy.Automated:
		return AutoDecision{Reason: "automation is off"}
	case in.OperationRunning:
		return AutoDecision{Reason: "another operation is running"}
	case in.Status.SyncStatus != SyncOutOfSync:
		return AutoDecision{Reason: "the application is " + in.Status.SyncStatus}
	}
	needsSync, onlyPrune, allPrune := false, true, len(in.Status.Resources) > 0
	for _, r := range in.Status.Resources {
		if r.Hook {
			continue
		}
		if r.SyncStatus != SyncExtraneous {
			allPrune = false
		}
		if r.SyncStatus == SyncSynced {
			continue
		}
		needsSync = true
		if r.SyncStatus != SyncExtraneous {
			onlyPrune = false
		}
	}
	if !needsSync {
		return AutoDecision{Reason: "nothing to change"}
	}
	if onlyPrune && !in.Policy.Prune {
		return AutoDecision{Reason: "only resources to prune, and automated prune is off"}
	}
	if allPrune && in.Policy.Prune {
		return AutoDecision{
			Reason:    "the sync would delete every resource",
			Condition: "Automated sync skipped: it would delete every resource of the application (the source renders nothing); sync by hand to confirm.",
		}
	}
	newState := in.Status.DesiredDigest != in.SyncedDigest
	if !newState && !in.Policy.SelfHeal {
		return AutoDecision{Reason: "the cluster drifted, and self-heal is off"}
	}
	if in.AutoDigest == in.Status.DesiredDigest && in.AutoAttempts > 0 {
		if in.AutoAttempts > in.Policy.Retries() {
			return AutoDecision{
				Reason: "retries exhausted",
				Condition: fmt.Sprintf("Automated sync to %s failed %d times; it waits for a new revision.",
					short(in.Status.Revision), in.AutoAttempts),
			}
		}
		if in.AutoNextAt != nil && in.Now.Before(*in.AutoNextAt) {
			at := *in.AutoNextAt
			return AutoDecision{Reason: "waiting for the next retry", RetryAt: &at}
		}
	}
	return AutoDecision{Sync: true, SelfHeal: !newState}
}

// AfterAttempt is the bookkeeping after an automated attempt: reset on
// success, one more failure with the next retry time otherwise.
func AfterAttempt(in AutoInput, succeeded bool) (digest string, attempts int, next *time.Time) {
	if succeeded {
		return "", 0, nil
	}
	attempts = 1
	if in.AutoDigest == in.Status.DesiredDigest {
		attempts = in.AutoAttempts + 1
	}
	at := in.Now.Add(Backoff(attempts))
	return in.Status.DesiredDigest, attempts, &at
}
