-- A rule's suggestion is either an exact replacement for the lines the
-- finding points at, or an example to adapt by hand (a line to add, text
-- with a placeholder such as `FROM node:<version>`). Only the first kind
-- may ever be committed by "Apply fixes" (CONTRACT.md §23.2). Until now
-- both were stored as `rule`. The one rule whose suggestion is an exact
-- replacement is the loosened version pin, which restores the removed line.

UPDATE findings
   SET suggestion_source = 'example'
 WHERE suggestion_source = 'rule'
   AND rule <> 'dependencies.pin_loosened';
