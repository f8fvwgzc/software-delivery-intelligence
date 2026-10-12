/**
 * Where focus goes when the element that had it is gone: a row's delete
 * button after the row was deleted, a dialog trigger that unmounted. The
 * page title is the stable landmark every page has; it is made focusable
 * on demand so the screen reader announces where the user landed.
 */
export function focusPageTitle() {
  const title = document.querySelector<HTMLElement>('main h1, h1')
  if (!title) return
  if (!title.hasAttribute('tabindex')) title.setAttribute('tabindex', '-1')
  title.focus({ preventScroll: true })
}
