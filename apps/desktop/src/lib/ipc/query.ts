// An empty query: every field matches all. Kept apart from api.ts so the
// mock backend can use it without a circular import.

import type { Query } from './bindings/Query';

export function emptyQuery(): Query {
  return {
    text: null,
    from_utc: null,
    to_utc: null,
    from_local: null,
    to_local: null,
    media: [],
    categories: [],
    exclude_categories: [],
    tags: [],
    event_id: null,
    cameras: [],
    lenses: [],
    formats: [],
    location: null,
    bbox: null,
    root_id: null,
    folder: null,
    recursive: false,
    min_rating: null,
    favorite: null,
    problems: null,
    review: [],
    immich: null,
    orientation: null,
    private: null,
    inbox: null,
    trashed: false,
    missing: false,
    staged: null,
    ids: null,
  };
}
