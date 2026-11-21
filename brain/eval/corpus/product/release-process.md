# Release process

We release to production on Tuesdays at 10:00 Icelandic time. There is a code freeze from Friday 12:00 until the following Tuesday, except for hotfixes approved by the on-call lead.

```bash
# cut the release branch
git switch -c release/2025-02-11 main
# run the full suite, including the slow integration tests
./gradlew check integrationTest
# tag and push; CI deploys to staging automatically
git tag v4.18.0 && git push --tags
```

After staging has been green for 24 hours the release is promoted. Rollbacks are done by redeploying the previous tag; database migrations must therefore always be backwards compatible for one release.
