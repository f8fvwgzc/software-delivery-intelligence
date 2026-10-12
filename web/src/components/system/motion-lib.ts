/*
 * The only module that imports `motion`. It is loaded dynamically by
 * PageTransition, so the animation runtime is its own chunk and never part
 * of the initial JS (CONTRACT.md §21). `m` + `domAnimation` is the small
 * feature set: transforms, opacity and presence, nothing more.
 */
export { domAnimation, LazyMotion, MotionConfig } from 'motion/react'
export * as m from 'motion/react-m'
