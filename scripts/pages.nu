const ARTIFACTS = [
    project.schema.json
    model.schema.json
]

def main []: nothing -> nothing {
    rojo-schema generate

    let files = $ARTIFACTS | each {|name|
        let path = [dist $name] | path join

        if not ($path | path exists) {
            error make $"missing artifact: ($path)"
        }

        $path
    }

    if ("site" | path exists) {
        rm --recursive site
    }

    mkdir site/latest

    for file in $files {
        cp $file site/latest
    }
}
