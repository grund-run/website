// Forest manifest for grund.run.
//
// Deployed to the existing homelab clusters (clank-dev, clank-prod) through
// the kjuulh organisation's Flux destinations, the same path the tiny
// services use, until grund runs on its own platform. Nothing here is specific
// to those clusters beyond the destination names: the image is a scratch
// binary with no volumes, secrets or database, so moving it is a destination
// change.
//
// TLS terminates in-cluster (cert-manager, per-host certificates); the public
// gateway only routes by SNI. See README.md "Getting traffic here".
package grund_website

project: {
	name:         "grund-website"
	organisation: "kjuulh"
	description:  "The server behind grund.run: a static site embedded in a scratch binary."
}

_destinationTypes: {
	flux: "forest/flux@1"
}

dependencies: {
	"forest/deployment": version:        "0.3.0"
	"kjuulh/kubernetes-app": version:    "0.1.13"
	"kjuulh/woodpecker-forest": version: "0.1.10"
}

forest: deployment: enabled: true

kjuulh: "kubernetes-app": {
	env: {
		dev: {
			destinations: [
				{destination: "flux-dev.*", type: _destinationTypes.flux},
			]
			config: {
				namespace: "dev"
				host:      "dev.grund.run"
				replicas:  1
				env_vars: {
					GRUND_WEBSITE_CANONICAL_ORIGIN: "https://dev.grund.run"
					// A pre-production host must never land in a search index.
					GRUND_WEBSITE_NOINDEX: "true"
				}
			}
		}

		prod: {
			destinations: [
				{destination: "flux-prod.*", type: _destinationTypes.flux},
			]
			config: {
				namespace: "prod"
				host:      "grund.run"
				// www gets its own route and certificate in the same Ingress
				// (kubernetes-app 0.1.13), so the server's 308 to the apex is
				// reachable.
				additional_hosts: ["www.grund.run"]
				replicas: 2
				env_vars: {
					GRUND_WEBSITE_CANONICAL_ORIGIN: "https://grund.run"
					GRUND_WEBSITE_REDIRECT_HOSTS:   "www.grund.run"
				}
			}
		}
	}

	config: {
		name:  "grund-website"
		image: "git.kjuulh.io/grund/website"
		// Overridden on every release with the main-<sha> tag CI published.
		// No image is ever tagged "main", so a render without the override
		// fails to pull instead of running something unknown.
		tag: "main"

		ports: [
			{name: "http", port: 8080},
		]

		// Every response is a copy from a table in the binary: no database,
		// no filesystem, no outbound calls. The limits leave headroom for a
		// designed site several megabytes large held in memory.
		resources: {
			requests: {
				cpu:    "10m"
				memory: "16Mi"
			}
			limits: {
				cpu:    "200m"
				memory: "64Mi"
			}
		}

		// Readiness reports the build revision and site digest; liveness
		// checks nothing, so no condition outside the process restarts it.
		health: {
			path:                  "/health/ready"
			liveness_path:         "/health/live"
			port:                  "http"
			initial_delay_seconds: 2
			period_seconds:        10
			timeout_seconds:       3
			failure_threshold:     3
		}
	}
}

// Generates .woodpecker/rollout.yaml (`forest run install`). The manual
// production job is off: production promotion belongs to Kasper, through
// forest, never to CI.
//
// The forest instance is forest.kjuulh.io. `server` is its gRPC API endpoint,
// api.forest.kjuulh.io: the web host forest.kjuulh.io answers gRPC calls
// with an HTML 404 (checked 2026-09-24). Pinned here rather than inherited from
// the component default, so that a change of default cannot move releases to
// another instance.
kjuulh: "woodpecker-forest": config: {
	server:          "https://api.forest.kjuulh.io"
	artifact_image:  "git.kjuulh.io/grund/website"
	manual_prod_job: false
}

commands: {
	check: ["./check.sh"]
	serve: ["cargo run"]
}
