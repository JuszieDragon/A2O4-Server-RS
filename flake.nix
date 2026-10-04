{
  description = "Development environment for A2O4-Server-RS";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable."1.97.1".default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        a2o4Package = pkgs.rustPlatform.buildRustPackage {
          pname = "A2O4-Server";
          version = "0.1.0";

          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          # Libraries required by the binary at link-time/runtime
          buildInputs = with pkgs; [
            openssl
          ];

          # Force pkg-config to locate the OpenSSL development files provided by Nix
          #PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";
        };
      in
      {
        packages.default = a2o4Package;

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            rustToolchain
            pkg-config
            gcc
          ];

          buildInputs = with pkgs; [
            openssl
          ];
        };
      }
    ) // {
      nixosModules.default = { config, lib, pkgs, ... }:
        with lib;
        let
          cfg = config.services.a2o4-server;
          configBlacklist = [ "enable" ];
          filteredConfig = builtins.removeAttrs cfg configBlacklist;
          configFile = (pkgs.formats.toml {}).generate "a2o4-config.toml" filteredConfig;
          package = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        in {
          options.services.a2o4-server = {
            enable = mkEnableOption "Enable A2O4-Server";

            port = mkOption {
              type = lib.types.port;
              default = 9797;
              description = "Port to listen on";
            };

            #TODO make sure to create this dir in the systemd service
            download_dir = mkOption {
              type = types.path;
              default = /var/lib/a2o4-server/downloads;
              description = "The directory to download fan-fiction in";
            };

            state_dir = mkOption {
              type = types.path;
              default = /var/lib/a2o4-server;
              description = "The directory to store service state, like the database file and cookies to preserve sessions";
            };

            ao3_login_file = mkOption {
              type = types.path;
              description = ''
                path to an env file containing ao3 login details.
                Example:
                  AO3_USERNAME=bob
                  AO3_PASSWORD=pass
              '';
            };

            default_format = mkOption {
              type = types.enum [ "Azw3" "Epub" "Mobi" "Pdf" "Html" ];
              default = "Epub";
              description = "The default format to download when the format is not specified in a request";
            };

            devices = mkOption {
              type = types.listOf (types.submodule {
                options = {
                  name = mkOption {
                    type = types.str;
                    description = "The name of the device";
                  };

                  ip = mkOption {
                    type = types.str;
                    description = "The IP used to connect to the device";
                  };

                  port = mkOption {
                    type = types.port;
                    description = "The port used to connect to the device";
                  };

                  username = mkOption {
                    type = types.str;
                    default = "";
                    description = "The username used to connect to the device";
                  };

                  password = mkOption {
                    type = types.str;
                    default = "";
                    description = "The password used to connect to the device";
                  };

                  upload_dir = mkOption {
                    type = types.str;
                    description = "The directory on the device to upload to";
                  };

                  client = mkOption {
                    type = types.enum [ "sftp" "crosspoint" ];
                    default = "sftp";
                    description = ''
                      The client to use to connect to the device.

                      sftp: Sftp is used to connect to the device, should work on any device that has an ssh server.
                      crosspoint: A custom websocket is used to connect to the device, only tested with the main crosspoint repo.
                    '';
                  };
                };
              });
            };

            fandom_map = mkOption {
              type = types.attrsOf types.str;
              default = {};
              description = "A set of key pairs for fandom mapping. the left value (name) will be replaced with the right value (value)";
            };

            fandom_filters = mkOption {
              type = with types;
                listOf (attrsOf (listOf str));
              default = {};
              description = ''
                A set of key pairs for fandom filtering. If the work/series has the fandom tag on the left value (name) the fandoms in right value (value) will be removed.
                The order of the filters matters, all filters are run for each work/series from top to bottom.
                There is also the special filter "*", this will remove all other fandom tags.

                Example value:
                [
                  {"Baldur's Gate" = ["Dungeons & Dragons" "Original Work"];}
                  {"Persona" = [ "Shin Megami Tensei" ];}
                  {"Dungeons & Dragons" = [ "Original Work" ];}
                  {"Original Work" = [ "*" ];}
                ]
              '';
            };

            user = mkOption {
              type = types.str;
              default = "a2o4";
              description = "User that A2O4 runs under";
            };

            group = mkOption {
              type = types.str;
              default = "a2o4";
              description = "Group that A2O4 runs under";
            };
          };

          config = mkIf cfg.enable {
            users.users = mkIf (cfg.user == "a2o4") {
              a2o4 = {
                inherit (cfg) group;
                isSystemUser = true;
              };
            };

            users.groups = mkIf (cfg.group == "a2o4") {
              a2o4 = { };
            };

            systemd.services.a2o4-server = {
              description = "A service to download from AO3 and upload fanfics to clients";
              after = [ "network-online.target" ];
              wants = [ "network-online.target" ];
              wantedBy = [ "multi-user.target" ];

              serviceConfig = {
                ExecStart = "${package}/bin/a2o4-server --config ${configFile} --ao3_login_file ${cfg.ao3_login_file}";
                #Restart = "always";

                StateDirectory = "a2o4-server";

                User = cfg.user;
                Group = cfg.group;
              };
            };

            assertions = lib.concatMap (device: [
              {
                assertion = device.name != null;
                message = "A device cannot be created without a name";
              }
              {
                assertion = device.ip != null;
                message = "Device ${device.name} cannot be created without an IP";
              }
              {
                assertion = device.port != null;
                message = "Device ${device.name} cannot be created without a port";
              }
              {
                assertion = device.upload_dir != null;
                message = "Device ${device.name} cannot be created without an upload directory";
              }
            ]) cfg.devices;
          };
        };
    };
}

