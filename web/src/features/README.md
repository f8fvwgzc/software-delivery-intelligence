# Features

One folder per product area. Each folder is self-contained:

```
<feature>/
├── api/
│   ├── keys.ts        query key factory: keys.all, keys.list(filters), keys.detail(id)
│   ├── queries.ts     queryOptions(...) built on the typed `api` client
│   └── mutations.ts   useX() hooks: useMutation + targeted invalidation
├── components/
│   └── <component>/
│       ├── index.tsx  the component
│       └── *.tsx      its parts, only used by index.tsx
├── lib/
│   └── labels.ts      status → label + StatusBadge tone, enum display names
└── pages/
    └── <page>.tsx     default export; lazy-loaded from app/router
```

Rules:

- Pages compose components; they do not fetch inline or hold business logic.
- Components receive data by props. Only `pages/` and a few live widgets call
  query hooks directly.
- No feature imports another feature's components. Shared things go to
  `components/shared`. Cross-feature data (e.g. the project crumb) goes
  through the other feature's `api/queries.ts`.
- Every async button: `<Button loading={mutation.isPending}>`.
