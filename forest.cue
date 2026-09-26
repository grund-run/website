// Forest manifest for grund.sh, grund's main site (grund.run redirects here
// until it becomes the domain for customer apps).
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
				host:      "dev.grund.sh"
				// dev.grund.run was the dev host before grund.sh became the main
				// site; it keeps working as a redirect.
				additional_hosts: ["dev.grund.run"]
				replicas: 1
				env_vars: {
					GRUND_WEBSITE_CANONICAL_ORIGIN: "https://dev.grund.sh"
					GRUND_WEBSITE_REDIRECT_HOSTS:   "dev.grund.run"
					// A pre-production host must never land in a search index.
					GRUND_WEBSITE_NOINDEX: "true"
					// Blog drafts (draft: true in blog/posts/) are served here only,
					// marked as drafts and noindex. Prod serves published posts.
					GRUND_WEBSITE_BLOG_DRAFTS: "true"
					// The newsletter sign-up, relayed to insights in dev. Off in
					// prod until insights and grund mail run there.
					GRUND_WEBSITE_NEWSLETTER: "true"
					// The nav's "Sign in" goes here (/sign-in, src/api.rs). Prod
					// sets none until the dashboard has a prod deployment, so
					// there /sign-in is a 404.
					GRUND_WEBSITE_APP_URL: "https://dev.app.grund.sh"
					// Page views for grund insights (src/insights.rs). The
					// namespace-local Service name, not a hostname: in-cluster,
					// plain http, never routed by an Ingress.
					GRUND_WEBSITE_INSIGHTS_URL: "http://grund-insights:8081"
					// Traefik sets this from the connection it saw. Until the edge
					// forwards client addresses it is the gateway's, which
					// insights ignores (not public).
					GRUND_WEBSITE_INSIGHTS_CLIENT_IP_HEADER: "X-Real-Ip"
				}
				// The bearer token insights in dev requires. A Secret created by
				// hand in the namespace; its value is never in this repository.
				secret_env: [
					{name: "GRUND_WEBSITE_INSIGHTS_TOKEN", secret: "grund-insights-ingest", key: "token"},
				]
			}
		}

		prod: {
			destinations: [
				{destination: "flux-prod.*", type: _destinationTypes.flux},
			]
			config: {
				namespace: "prod"
				// grund.sh is the main site (Kasper, 2026-09-24). grund.run
				// becomes the domain for customer apps; until then its apex and
				// www redirect here, as does www.grund.sh. All four names share
				// one Ingress and certificate (kubernetes-app 0.1.13).
				host: "grund.sh"
				additional_hosts: ["www.grund.sh", "grund.run", "www.grund.run"]
				replicas: 2
				env_vars: {
					GRUND_WEBSITE_CANONICAL_ORIGIN: "https://grund.sh"
					GRUND_WEBSITE_REDIRECT_HOSTS:   "www.grund.sh,grund.run,www.grund.run"
					// Page views for grund insights (src/insights.rs), as in dev:
					// the namespace-local Service, plain http, in-cluster only.
					GRUND_WEBSITE_INSIGHTS_URL:              "http://grund-insights:8081"
					GRUND_WEBSITE_INSIGHTS_CLIENT_IP_HEADER: "X-Real-Ip"
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
