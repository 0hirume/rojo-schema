const ARTIFACTS = [
    project.schema.json
    model.schema.json
    manifest.json
    coverage.json
]

def main []: nothing -> nothing {
    gh repo clone $env.ROJO_REPOSITORY sources/rojo -- --depth 1

    let tracker = [sources client-tracker] | path join
    mkdir $tracker

    let revision = (
        gh api $"repos/($env.TRACKER_REPOSITORY)/commits/roblox" --jq .sha
        | str trim
    )

    $revision | save --force ($tracker | path join .revision)

    for file in [Full-API-Dump.json version.txt] {
        (gh
            api
            $"repos/($env.TRACKER_REPOSITORY)/contents/($file)?ref=($revision)"
            --header
            "Accept: application/vnd.github.raw+json"
        ) | save --force ($tracker | path join $file)
    }

    (gh
        repo
        clone
        $env.DOCS_REPOSITORY
        sources/creator-docs
        --
        --depth
        1
        --filter=blob:none
        --sparse
    )

    git -C sources/creator-docs sparse-checkout set content/en-us/reference/engine

    let archive = "rojo-schema-x86_64-unknown-linux-gnu"
    gh release download --repo $env.REPOSITORY --pattern $"($archive)*"
    sha256sum --check $"($archive).sha256"
    mkdir bin
    tar -xzf $"($archive).tar.gz" -C bin

    (bin/rojo-schema
        generate
        --rojo
        sources/rojo
        --docs
        sources/creator-docs
        --tracker
        sources/client-tracker
    )

    let files = $ARTIFACTS | each {|name|
        let path = [dist $name] | path join

        if not ($path | path exists) {
            error make $"missing artifact: ($path)"
        }

        $path
    }

    let store = [$env.RUNNER_TEMP $"rojo-schema-snapshots-($env.GITHUB_RUN_ID)"] | path join
    gh auth setup-git

    let remote = $"https://github.com/($env.REPOSITORY).git"
    let branch = (git ls-remote --exit-code --heads $remote $env.SNAPSHOT_BRANCH | complete)

    if $branch.exit_code == 0 {
        (gh
            repo
            clone
            $env.REPOSITORY
            $store
            --
            --branch
            $env.SNAPSHOT_BRANCH
            --depth
            1
            --single-branch
        )
    } else if $branch.exit_code == 2 {
        git init --initial-branch $env.SNAPSHOT_BRANCH $store
        git -C $store remote add origin $remote
    } else {
        error make $"checking snapshot branch failed with exit code ($branch.exit_code): ($branch.stderr | str trim)"
    }

    let hash = $files | each {|path| open --raw $path } | str join (char nul) | hash sha256
    let index_path = [$store index.json] | path join

    let index = if ($index_path | path exists) {
        open $index_path
    } else {
        {latest: null, snapshots: []}
    }

    let previous = $index | get --optional latest.sha256

    if $previous == $hash {
        print $"schema unchanged: ($hash)"
    } else {
        let now = date now | date to-timezone "+0000"
        let created_at = $now | format date %Y-%m-%dT%H:%M:%SZ
        let stamp = $now | format date %Y-%m-%d-%H%M%SZ
        let identifier = $"($stamp)-($hash | str substring 0..<12)"
        let destination = [$store $identifier] | path join
        mkdir $destination

        for artifact in $files {
            cp $artifact $destination
        }

        let entry = {
            id: $identifier
            createdAt: $created_at
            sha256: $hash
            artifacts: $ARTIFACTS
        }

        let snapshots = $index | get --optional snapshots | default []

        {
            latest: $entry
            snapshots: ($snapshots | prepend $entry)
        } | to json --indent 2 | save --force $index_path

        git -C $store config user.name "github-actions[bot]"
        git -C $store config user.email "41898282+github-actions[bot]@users.noreply.github.com"
        git -C $store add --all
        git -C $store commit -m $"snapshot: ($identifier)"
        git -C $store push --set-upstream origin $env.SNAPSHOT_BRANCH
    }

    if ("site" | path exists) {
        rm --recursive site
    }

    mkdir site/latest site/snapshots

    for file in $files {
        cp $file site/latest
    }

    for file in (ls $store | get name) {
        cp --recursive $file site/snapshots
    }

    cp .github/pages.html site/index.html
}
