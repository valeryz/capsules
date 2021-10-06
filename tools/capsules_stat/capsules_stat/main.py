"""
Collecting stats from running capsules.
"""
import os
import os.path
import time

from absl import app
from absl import flags
from gitlab import Gitlab

FLAGS = flags.FLAGS

flags.DEFINE_string("gitlab_uri", "https://gitlab.com", "URI of the Gitlab instance")
flags.DEFINE_integer("gitlab_project_id", 25333072, "Gitlab Project ID")
flags.DEFINE_string("traces", "traces", "Where to place traces")
flags.DEFINE_integer("start_page", 1, "Starting page")


def past_job_ids(pages):
    gl = Gitlab(FLAGS.gitlab_uri, private_token=os.environ["GITLAB_ACCESS_TOKEN"])
    project = gl.projects.get(FLAGS.gitlab_project_id)
    os.makedirs(FLAGS.traces, exist_ok=True)
    for page in range(FLAGS.start_page, FLAGS.start_page+100):
        print ("Page: ", page)
        jobs = project.jobs.list(page=page, per_page=100)
        for job in jobs:
            if job.name not in ("cargo-build-debug-linux",
                                "cargo-build-release-linux-native",
                                "generic-guest-os-diskimg"):
                continue
            with open(os.path.join(FLAGS.traces, str(job.id)), "w+") as out:
                out.write(job.trace().decode("utf-8", "ignore"))
                time.sleep(1)

def run_stats(unused_argv):
    past_job_ids(1000)
            
def main():
    app.run(run_stats)
