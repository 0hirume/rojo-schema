def fail [message: string]: nothing -> error {
    error make {
        msg: $message
        label: {
            text: $message
            span: (metadata $message).span
        }
    }
}

def completed [program: string, ...arguments: string]: nothing -> record {
    run-external $program ...$arguments | complete
}

def checked [program: string, ...arguments: string]: nothing -> nothing {
    run-external $program ...$arguments

    let exit_code = $env.LAST_EXIT_CODE

    if $exit_code != 0 {
        fail $"($program) failed with exit code ($exit_code)"
    }
}

def main []: nothing -> nothing {
    checked gh auth setup-git
    let repository_url = $"https://github.com/($env.REPOSITORY).git"
    let remote = (completed git ls-remote "--exit-code" "--heads" $repository_url gh-pages)

    if $remote.exit_code == 0 {
        (checked
            gh
            repo
            clone
            $env.REPOSITORY
            site
            "--"
            "--branch"
            gh-pages
            "--depth"
            "1"
        )
    } else if $remote.exit_code == 2 {
        checked git init site
        checked git "-C" site switch "--orphan" gh-pages
        checked git "-C" site remote add origin $repository_url
    } else {
        fail ($remote.stderr | str trim)
    }

    let latest = [site latest] | path join
    try {
        if ($latest | path exists) {
            rm --recursive $latest
        }

        mkdir $latest

        for file in (glob "dist/*") {
            cp --recursive $file $latest
        }

        touch ([site .nojekyll] | path join)
    } catch {|error| fail $error.msg }

    checked git "-C" site add .
    let changes = (completed git "-C" site diff "--cached" "--quiet")

    if $changes.exit_code == 0 {
        return
    }

    if $changes.exit_code != 1 {
        fail ($changes.stderr | str trim)
    }

    checked git "-C" site config user.name "github-actions[bot]"
    (checked
        git
        "-C"
        site
        config
        user.email
        "41898282+github-actions[bot]@users.noreply.github.com"
    )
    checked git "-C" site commit "-m" "deploy latest"
    checked git "-C" site push origin gh-pages
}
