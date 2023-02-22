use crate::xdg::{DesktopFile, Icon, IconType};
use color_eyre::eyre::{eyre, Result};
use glam::Quat;
use mint::Vector3;
use nix::unistd::{execv, fork};
use stardust_xr_molecules::{
	fusion::{
		client::{Client, FrameInfo, RootHandler},
		core::values::Transform,
		drawable::{MaterialParameter, Model, ResourceID},
		fields::BoxField,
		node::NodeType,
		spatial::Spatial,
		startup_settings::StartupSettings,
	},
	GrabData, Grabbable,
};
use std::{f32::consts::PI, ffi::CStr, sync::Arc};
use tween::{QuartInOut, Tweener};
use ustr::ustr;

fn model_from_icon(parent: &Spatial, icon: &Icon) -> Result<Model> {
	return match &icon.icon_type {
		IconType::Png(path) => {
			let model = Model::create(
				parent,
				Transform::from_rotation(Quat::from_rotation_y(PI)),
				&ResourceID::new_namespaced("protostar", "cartridge"),
			)?;
			model.set_material_parameter(
				0,
				"diffuse",
				MaterialParameter::Texture(ResourceID::Direct(path.clone())),
			)?;
			Ok(model)
		}
		IconType::Gltf(path) => Ok(Model::create(
			parent,
			Transform::from_scale([0.05; 3]),
			&ResourceID::new_direct(path)?,
		)?),
		_ => panic!("asd"),
	};
}

pub struct ProtoStar {
	client: Arc<Client>,
	grabbable: Grabbable,
	field: BoxField,
	icon: Model,
	icon_shrink: Option<Tweener<f32, f64, QuartInOut>>,
	execute_command: String,
}
impl ProtoStar {
	pub fn create_from_desktop_file(parent: &Spatial, desktop_file: DesktopFile) -> Result<Self> {
		// dbg!(&desktop_file);
		let raw_icons = desktop_file.get_raw_icons();
		let mut icon = raw_icons
			.clone()
			.into_iter()
			.find(|i| match i.icon_type {
				IconType::Gltf(_) => true,
				_ => false,
			})
			.or(
			raw_icons
				.into_iter()
				.max_by_key(|i| i.size)
			);

		match icon{
			Some(i) => {
				icon = match i.process(128) {
					Ok(i) => Some(i),
					_ => None,
			}},
			None => {},
		}

		Self::new_raw(
			parent,
			icon,
			desktop_file.command.ok_or_else(|| eyre!("No command"))?,
		)
	}
	pub fn new_raw(parent: &Spatial, icon: Option<Icon>, execute_command: String) -> Result<Self> {
		let field = BoxField::create(
			parent,
			Transform::default(),
			match icon.as_ref() {
				Some(_) => [0.05, 0.0665, 0.005],
				_ => [0.05; 3],
			}
			.into(),
		)?;
		let grabbable = Grabbable::new(
			parent,
			Transform::default(),
			&field,
			GrabData {
				max_distance: 0.025,
			},
		)?;
		field.set_spatial_parent(grabbable.content_parent())?;
		let icon = icon
			.map(|i| model_from_icon(grabbable.content_parent(), &i))
			.unwrap_or_else(|| {
				Ok(Model::create(
					grabbable.content_parent(),
					Transform::from_scale([0.05; 3]),
					&ResourceID::new_namespaced("protostar", "default_icon"),
				)?)
			})?;
		Ok(ProtoStar {
			client: parent.client()?,
			grabbable,
			field,
			icon,
			icon_shrink: None,
			execute_command,
		})
	}
	pub fn content_parent(&self) -> &Spatial {
		self.grabbable.content_parent()
	}
}
impl RootHandler for ProtoStar {
	fn frame(&mut self, info: FrameInfo) {
		self.grabbable.update();

		if let Some(icon_shrink) = &mut self.icon_shrink {
			if !icon_shrink.is_finished() {
				let scale = icon_shrink.move_by(info.delta);
				self.icon
					.set_scale(None, Vector3::from([scale; 3]))
					.unwrap();
			} else {
				self.client.stop_loop();
			}
		} else if self.grabbable.grab_action().actor_stopped() {
			let startup_settings = StartupSettings::create(&self.field.client().unwrap()).unwrap();
			self.icon
				.set_spatial_parent_in_place(self.client.get_root())
				.unwrap();
			self.grabbable
				.content_parent()
				.set_rotation(
					Some(&self.field.client().unwrap().get_root()),
					Quat::IDENTITY,
				)
				.unwrap();
			startup_settings
				.set_root(self.grabbable.content_parent())
				.unwrap();
			self.icon_shrink = Some(Tweener::quart_in_out(1.0, 0.0, 0.25));
			let future = startup_settings.generate_startup_token().unwrap();
			let executable = dbg!(self.execute_command.clone());
			//TODO: split the executable string for  the args
			tokio::task::spawn(async move {
				std::env::set_var("STARDUST_STARTUP_TOKEN", future.await.unwrap());
				if unsafe { fork() }.unwrap().is_parent() {
					println!("Launching \"{}\"...", &executable);
					execv::<&CStr>(
						ustr("/bin/sh").as_cstr(),
						&[
							ustr("/bin/sh").as_cstr(),
							ustr("-c").as_cstr(),
							ustr(&executable).as_cstr(),
						],
					)
					.unwrap();
				}
			});
		}
	}
}
