use color_eyre::eyre::Result;
use glam::{EulerRot, Quat, Vec3};
use protostar::{
	application::Application,
	xdg::{DesktopFile, Icon, IconType},
};
use stardust_xr_fusion::{
	core::values::{ResourceID, Vector3},
	drawable::{
		MaterialParameter, Model, ModelPartAspect, Text, TextBounds, TextFit, TextStyle, XAlign,
		YAlign,
	},
	fields::{CylinderShape, Field, Shape},
	node::NodeType,
	root::FrameInfo,
	spatial::{Spatial, SpatialAspect, SpatialRefAspect, Transform},
};
use stardust_xr_molecules::{Grabbable, GrabbableSettings};
use std::f32::consts::PI;
use tween::{QuartInOut, Tweener};

use crate::{State, ACTIVATION_DISTANCE, APP_SIZE, DEFAULT_HEX_COLOR};

// Model handling
fn model_from_icon(parent: &Spatial, icon: &Icon) -> Result<Model> {
	match &icon.icon_type {
		IconType::Png => {
			let t = Transform::from_rotation_scale(
				Quat::from_rotation_x(PI / 2.0) * Quat::from_rotation_y(PI),
				[APP_SIZE / 2.0; 3],
			);

			let model = Model::create(
				parent,
				t,
				&ResourceID::new_namespaced("protostar", "hexagon/hexagon"),
			)?;
			model
				.part("Hex")?
				.set_material_parameter("color", MaterialParameter::Color(DEFAULT_HEX_COLOR))?;
			model.part("Icon")?.set_material_parameter(
				"diffuse",
				MaterialParameter::Texture(ResourceID::Direct(icon.path.clone())),
			)?;
			Ok(model)
		}
		IconType::Gltf => Ok(Model::create(
			parent,
			Transform::from_scale([0.05; 3]),
			&ResourceID::new_direct(icon.path.clone())?,
		)?),
		_ => panic!("Invalid Icon Type"),
	}
}

pub struct App {
	application: Application,
	parent: Spatial,
	position: Vector3<f32>,
	grabbable: Grabbable,
	_field: Field,
	// field_lines: Lines,
	icon: Model,
	label: Option<Text>,
	grabbable_shrink: Option<Tweener<f32, f64, QuartInOut>>,
	grabbable_grow: Option<Tweener<f32, f64, QuartInOut>>,
	grabbable_move: Option<Tweener<f32, f64, QuartInOut>>,
}
impl App {
	pub fn create_from_desktop_file(
		parent: &Spatial,
		position: impl Into<Vector3<f32>>,
		desktop_file: DesktopFile,
		state: &State,
	) -> Result<Self> {
		let position = position.into();
		let field = Field::create(
			parent,
			Transform::identity(),
			Shape::Cylinder(CylinderShape {
				length: 0.01,
				radius: APP_SIZE / 2.0,
			}),
		)?;
		// let circle = circle(32, 0.0, APP_SIZE / 2.0).thickness(0.001);
		// let field_lines = Lines::create(
		// 	&field,
		// 	Transform::identity(),
		// 	&[
		// 		circle
		// 			.clone()
		// 			.transform(Mat4::from_translation([0.0, 0.0, 0.005].into())),
		// 		circle
		// 			.clone()
		// 			.transform(Mat4::from_translation([0.0, 0.0, -0.005].into())),
		// 	],
		// )?;
		let application = Application::create(desktop_file)?;
		let icon = application.icon(128, false);
		let grabbable = Grabbable::create(
			parent,
			Transform::from_translation(position),
			&field,
			GrabbableSettings {
				max_distance: 0.05,
				zoneable: false,
				..Default::default()
			},
		)?;
		if !state.unfurled {
			grabbable.set_enabled(false)?;
		}
		grabbable.content_parent().set_spatial_parent(parent)?;
		field.set_spatial_parent(grabbable.content_parent())?;
		let icon = icon
			.map(|i| model_from_icon(grabbable.content_parent(), &i))
			.unwrap_or_else(|| {
				Ok(Model::create(
					grabbable.content_parent(),
					Transform::from_rotation_scale(
						Quat::from_rotation_x(PI / 2.0) * Quat::from_rotation_y(PI),
						[APP_SIZE * 0.5; 3],
					),
					&ResourceID::new_namespaced("protostar", "hexagon/hexagon"),
				)?)
			})?;
		if !state.unfurled {
			icon.set_enabled(false)?;
		}

		let label_style = TextStyle {
			character_height: APP_SIZE * 2.0,
			bounds: Some(TextBounds {
				bounds: [1.0; 2].into(),
				fit: TextFit::Wrap,
				anchor_align_x: XAlign::Center,
				anchor_align_y: YAlign::Center,
			}),

			text_align_x: XAlign::Center,
			text_align_y: YAlign::Center,
			..Default::default()
		};
		let label = application.name().and_then(|name| {
			Text::create(
				&icon,
				Transform::from_translation_rotation(
					[0.0, 0.1, -(APP_SIZE * 4.0)],
					Quat::from_rotation_x(PI * 0.5),
				),
				name,
				label_style,
			)
			.ok()
		});
		if !state.unfurled {
			if let Some(label) = label.as_ref() {
				label.set_enabled(false)?;
			}
		}

		Ok(App {
			parent: parent.alias(),
			position,
			grabbable,
			_field: field,
			// field_lines,
			label,
			application,
			icon,
			grabbable_shrink: None,
			grabbable_grow: None,
			grabbable_move: None,
		})
	}
	pub fn content_parent(&self) -> &Spatial {
		self.grabbable.content_parent()
	}
	pub fn apply_state(&mut self, state: &State) {
		self.grabbable.set_enabled(state.unfurled).unwrap();
		if state.unfurled {
			self.icon.set_enabled(true).unwrap();
			if let Some(label) = self.label.as_ref() {
				label.set_enabled(true).unwrap()
			}
			self.grabbable_move = Some(Tweener::quart_in_out(0.0001, 1.0, 0.25));
		} else {
			self.grabbable_move = Some(Tweener::quart_in_out(1.0, 0.0001, 0.25)); //TODO make the scale a parameter
		}
	}

	pub fn frame(&mut self, info: &FrameInfo, state: &State) {
		let _ = self.grabbable.update(info);

		if let Some(grabbable_move) = &mut self.grabbable_move {
			if !grabbable_move.is_finished() {
				let scale = grabbable_move.move_by(info.delta.into());
				self.grabbable
					.content_parent()
					.set_relative_transform(
						&self.parent,
						Transform::from_translation(Vec3::from(self.position) * scale),
					)
					.unwrap();
			} else {
				if grabbable_move.final_value() == 0.0001 {
					self.icon.set_enabled(false).unwrap();
					if let Some(label) = self.label.as_ref() {
						label.set_enabled(false).unwrap()
					}
				}
				self.grabbable_move = None;
			}
		}
		if let Some(grabbable_shrink) = &mut self.grabbable_shrink {
			if !grabbable_shrink.is_finished() {
				let scale = grabbable_shrink.move_by(info.delta.into());
				self.grabbable
					.content_parent()
					.set_relative_transform(&self.parent, Transform::from_scale([scale; 3]))
					.unwrap();
			} else {
				self.grabbable
					.content_parent()
					.set_spatial_parent(&self.parent)
					.unwrap();
				if state.unfurled {
					self.grabbable_grow = Some(Tweener::quart_in_out(0.0001, 1.0, 0.25));
					self.grabbable.cancel_angular_velocity();
					self.grabbable.cancel_linear_velocity();
				}
				self.grabbable_shrink = None;
				self.grabbable
					.content_parent()
					.set_relative_transform(
						&self.parent,
						Transform::from_translation(self.position),
					)
					.unwrap();
				self.grabbable
					.content_parent()
					.set_relative_transform(&self.parent, Transform::from_rotation(Quat::default()))
					.unwrap();
				self.icon
					.set_local_transform(Transform::from_rotation(
						Quat::from_rotation_x(PI / 2.0) * Quat::from_rotation_y(PI),
					))
					.unwrap();
			}
		} else if let Some(grabbable_grow) = &mut self.grabbable_grow {
			if !grabbable_grow.is_finished() {
				let scale = grabbable_grow.move_by(info.delta.into());
				self.grabbable
					.content_parent()
					.set_relative_transform(&self.parent, Transform::from_scale([scale; 3]))
					.unwrap();
			} else {
				self.grabbable
					.content_parent()
					.set_spatial_parent(&self.parent)
					.unwrap();
				self.grabbable_grow = None;
			}
		} else if self.grabbable.grab_action().actor_stopped() {
			self.grabbable_shrink = Some(Tweener::quart_in_out(APP_SIZE * 0.5, 0.0001, 0.25));

			let application = self.application.clone();
			let space = self.content_parent().alias();
			let parent = self.parent.alias();

			//TODO: split the executable string for the args
			tokio::task::spawn(async move {
				let distance_vector = space
					.get_transform(&parent)
					.await
					.unwrap()
					.translation
					.unwrap();
				let distance = Vec3::from(distance_vector).length_squared();

				if distance > ACTIVATION_DISTANCE {
					let client = space.node().client().unwrap();
					let space_rot = space
						.get_transform(client.get_root())
						.await
						.unwrap()
						.rotation
						.unwrap();
					let (_, y_rot, _) = Quat::from(space_rot).to_euler(EulerRot::XYZ);
					let _ = space.set_relative_transform(
						client.get_root(),
						Transform::from_rotation_scale(Quat::from_rotation_y(y_rot), [1.0; 3]),
					);
					let _ = application.launch(&space);
				}
			});
		}
	}
}
