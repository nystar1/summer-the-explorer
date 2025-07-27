set -euo pipefail # lets not let it keep running if something fails bruh

cargo zigbuild -p summer-the-explorer --release --target x86_64-unknown-linux-gnu

DEPLOY_DIR=".deploy_temp" # throw in all our deploy files here
git worktree add "$DEPLOY_DIR" HEAD

(
  cd "$DEPLOY_DIR"
  git rm -rf . 2>/dev/null || true
  cp -f ../target/x86_64-unknown-linux-gnu/release/summer-the-explorer .
  cp -f ../libonnxruntime.so .
  cp -f ../Procfile .
  git add summer-the-explorer libonnxruntime.so Procfile
  git commit -m "chore: temp build for Heroku deploy"
  git push --force heroku HEAD:main
)

git worktree remove "$DEPLOY_DIR" --force
rm -rf "$DEPLOY_DIR"

echo "deployed to heroku!! and undid the commits lol"
