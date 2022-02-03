# Capsules

Capsules aims to encapsulate large and small build and test steps in a reproducible, isolated and cacheable
way. One instantiation is a `capsule`. Since a typical project will have many such instantiations in
multiple places, the name of the project is in plural: `Capsules`.

Capsules attempt to be minimally intrusive into the build process, and should be compatible with any build process: cargo, npm, make, Bazel etc.  It is easy to opt in to and opt out of capsules.


# Invocation

Invoke capsule with the command you wish to wrap after a double dash (`--`):

```
capsule <... capsule options ...> -- command <... command arguments ...>
```


The capsule will look up an entry in the cache, keyed by a hash of its inputs. If there is a match,
the capsule will download cached results instead of invoking the given command. If there is no match,
the capsule will execute the specified command, and store the specified results in the cache for future
matches.

A capsule can be configured as "placebo". In this case, it will perform the lookup, but will not use
the results even if there is cache hit. Instead, it will compare the cache hit with the results of
the executed command, and complain if they mismatch. In placebo mode, the results will still be
uploaded to the cache after the command is executed. This could be used for non-determinism
detection or for pre-populating the cache.

The cache backend is currently S3 or a compatible storage. Modular architecture allows adding other
storages in the future.

Capsules try to be very conservative with error handling. This is part of the philosophy to be
minimally intrusive. If anything goes wrong (cache is down, networking timeouts, misconfiguration),
capsules default to just running the requested command, allowing build pipelines to proceed despite
capsule infrastructure errors.


# Configuration

Capsules are configured in four places:

  * `${HOME}/.capsules.toml` configures all capsules. The file is read first, if exists, and can be used to set the defaults (such as S3 configuration).
  * `Capsule.toml` in the current directory configures either one capsule if there's just one, or multiple capsules in the current directory. If the capsule has many inputs, it is convenient to specify them in Capsule.toml.
  * `CAPSULE_ARGS` environment variable: used to conveniently provide the same arguments as command line, but once for all the capsules in the child processes. Best used in a CI pipeline configuration to propagate configuration that is specific to a CI pipeline and is identical for all capsule instances.
  * Command line arguments: the most specific configuration for a given capsule instnance.


# Options

Options below could be provided either as command line arguments, or as entries in the TOML files (without the leading `--`):

## Capsule Invocation Options

  * `--capsule_id (-c)`: ID of the capsule instance. All caching is done withing a specific capsule instance identified by this ID. This option is required in most invocations of capsule, except `--passive` and `--inputs_hash`. If capsule ID is not specified on the command line,

  * `--passive`: Used to disable capsule functionality. In this mode, the capsule does nothing except calling the wrapped command - it doesn't look up in the cache, doesn't write observabiltiy logs etc. It is convenient to set in CAPSULE_ARGS on CI when you need to disable all capsules.

  * `--placebo (-p)`: Run capsule in placebo mode, where it does all the steps except actually using the cached result on cache hit. It will always run the wrapped command, and it will store the outputs in the cache. Additionally, it will compare the real outputs hashes with the outputs hashes from the cache hit and complain to stderr and to Honeycomb if there is non-determinism.  Another way to run a capsule in placebo mode is to name the binary `placebo` using a hard or symbolic link.

  * `--inputs_hash`: Run capsule in inputs hash calculation mode. It will read its inputs hash, print it to the stdout and exit. There will be no cache lookup. This is used to determine the `Build ID` - a hash of inputs of some particular output, to be used outside the context of the capsule itself.

  * `--verbose (-v)`: Add more verbosity, will print inputs/outputs hashes per file.

## Specifying Inputs and Outputs

  * `--input (-i)`: Specify an input file. There could be multiple `-i` options. In TOML, it should be an array. Globs are supported, e.g. `-i "../gitlab-runner-tmp/**/*"`, or, to select all files below current directory, use `-i "**/*"`.

  * `--tool_tag (-t)`: Specify a tool tag. Tool tags are opaque strings that are added to the hash of the inputs, that are not representable as an input file. For example, hash of the docker image, compiler version, and so on. There could be multiple `-i` options. In TOML, it should be an array.

  * `--output (-o)`: Specify an output file. This is an artifact produced by the command we are wrapping. The path will be recorded in the cache as is. Therefore it should likely be a relative path, unless the invocation of the given capsule ID is always performed in the same directory. This may change in the future, if capsule supports project root relative paths. In TOML, it should be an array.  Globs are also supported for `-o`.

  * `--capture_stdout`: Whether stdout should be captured as one of the output files and returned on cache hit. Not implemented at the moment.

  * `--capture_stdout`: Whether stderr should be captured as one of the output files and returned on cache hit. Not implemented at the moment.


## Caching Options

  * `--backend (-b)`: Which backend to use. Possible options are `s3` and `dummy` (default).

  * `--cache_failures (-f)`: Whether to use cached failed invocations of the command. The default is false, if the cache hit finds the non-zero exit status, the command will be run again. This is useful for caching tests, and detecting their flakiness, as this will be triggered as non-determinism.

  * `--capsule_job (-j)`: Some opaque representaiton of the original capsule invocation from which the cache entry is taken. If the capsule ends up writing a cache entry, it will store this parameter in the cache entry. On cache hit, capsule will log this ID. This will allow to investigate invalid cache hits, by understanding where the cache entry is coming from. In GitLab, it makes sense to set this variable to the URL of the job.


## S3 Options

  * `--s3_bucket`: The bucket used for cache entries.

  * `--s3_bucket_objects`: The bucket used for objects (blobs) as Content Addressable Storage (CAS). Capsules use separate bucktets for key/value lookup of metainformation and for the blobs themselves. This allows to make different choices for the setup of those buckets.

  * `--s3_endpoint`: S3 endpoint

  * `--s3_region`: S3 region

Authentication for S3 is set in the same way as in AWS CLI, using `~/.aws/credentials`.  See https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-files.html.


## Observability Options

Currently, capsules support logging the results of their operation to Honeycomb (http://honeycomb.io) for anaylsis and alerting. Other backends could be added as needed.

  * `--honeycomb_dataset`: Honeycomb Dataset where the results will be stored.

  * `--honeycomb_token`: Authentication token for Honeycomb writes.

  * `--honeycomb_trace_id`: Trace ID for Honeycomb. It is convenient to set it equal to the capsule ID.

  * `--honeycomb_parent_id`: Parent ID for this Honeycomb trace. It is convenient to set it to the Pipeline ID in CI.

  * `--honeycomb_kv`: Additional opaque string in the format `key=value` that will be added to the honeycomb entry for this capsule invocation. For example, it used to log the current git branch on CI: `--honeycomb_kv=branch='${CI_COMMIT_BRANCH:-}'`.


## Misc Options

  * `--inputs_hash_var`: set the name of the environmental variable in which capsules will publish the inputs hash. When the capsule runs a command, the command sees the hash of its inputs in a variable `CAPSULE_INPUTS_HASH`. This option allows to customize this variable name.  For example, for many commands that depend on some version string, this could be set to `VERSION`, or even `GIT_REVISION` to fake a git revision with a build id.


# Roadmap

The roadmap for Capsules consists of four milestones:

  1. Placebo (achieved) - calculate inputs/outputs hashes, collect data via observability.
  2. Blue Pill (achieved) - capsules can store to and retrieve results from the cache.
  3. Organe Pill (planned) - capsules can sandbox the build process, so that one can always be sure that the dependencies are specified correctly. As it is, one has to be careful with maintaining dependencies (the best way for this would be to find those dependencies from the build system in use, e.g. from Cargo itself).
  4. Red Pill (planned) - capsules can apply full hermeticity and resource constraints on the process, and enables remote build.

With these milestones achieved, capsules will be much less intrusive than Bazel or Nix, so that developers can still use their standard build systems, but still get the benefits of caching, better capacity planning and resource utilization, with just one small Rust program.

Opting out: if capsules are used intensively in some build pipeline, enough of dependency information would be collected to make a migration to e.g. Bazel much easier. Therefore one can see Capsules as intermediate step for Bazel adoption.
